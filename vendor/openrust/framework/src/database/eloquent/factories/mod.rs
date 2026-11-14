//! Eloquent model factories for seeders and tests.

use std::{any::Any, sync::Arc};

use tokio::sync::OnceCell;

use crate::database::{Connection, DB, DatabaseError, DatabaseFuture, DatabaseResult, Value};

use super::{Attributes, Collection, Model, casts, relations::BelongsToMany};

type Definition = dyn Fn(&Connection) -> DatabaseResult<Attributes> + Send + Sync;
type State = dyn Fn(&Attributes, usize) -> Attributes + Send + Sync;
type Callback<M> = dyn Fn(&mut M) -> DatabaseResult<()> + Send + Sync;
type Relationship<M> = dyn for<'a> Fn(
        &'a Connection,
        &'a mut M,
        &'a [RecycledModel],
    ) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;

mod faker;
mod relationships;

pub use faker::{Faker, fake};

pub use relationships::{FactoryChild, FactoryParent};

type Parent<M> = dyn for<'a> Fn(
        &'a Connection,
        &'a mut M,
        &'a ParentCache,
        &'a [RecycledModel],
    ) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;
type SharedModel = Arc<dyn Any + Send + Sync>;
type ParentCache = OnceCell<SharedModel>;

#[derive(Clone)]
struct RecycledModel {
    connection: Connection,
    model: SharedModel,
}

fn recycled_owner<R: Model>(
    pool: &[RecycledModel],
    connection: &Connection,
) -> Option<SharedModel> {
    let candidates: Vec<_> = pool
        .iter()
        .filter(|entry| entry.model.is::<R>() && entry.connection.same_query_source(connection))
        .collect();
    fastrand::choice(candidates).map(|entry| entry.model.clone())
}

/// Associates a model with the factory definition chosen by its application.
pub trait HasFactory: Model {
    fn factory() -> Factory<Self>;
}

/// A model definition with ordered attribute states and relationship callbacks.
pub struct Factory<M: Model> {
    definition: Box<Definition>,
    states: Vec<Box<State>>,
    recycled: Vec<RecycledModel>,
    parents: Vec<Box<Parent<M>>>,
    children: Vec<Box<Relationship<M>>>,
    after_making: Vec<Box<Callback<M>>>,
    after_creating: Vec<Box<Callback<M>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's factory API")]
impl<M: Model> Factory<M> {
    pub fn new(definition: impl Fn() -> Attributes + Send + Sync + 'static) -> Self {
        Self::from_definition(move |_| Ok(definition()))
    }

    /// Defines model attributes using the application's configured Faker locale.
    pub fn withFaker(definition: impl Fn(&Faker) -> Attributes + Send + Sync + 'static) -> Self {
        Self::from_definition(move |connection| Ok(definition(&Faker::for_connection(connection)?)))
    }

    fn from_definition(
        definition: impl Fn(&Connection) -> DatabaseResult<Attributes> + Send + Sync + 'static,
    ) -> Self {
        Self {
            definition: Box::new(definition),
            states: Vec::new(),
            recycled: Vec::new(),
            parents: Vec::new(),
            children: Vec::new(),
            after_making: Vec::new(),
            after_creating: Vec::new(),
        }
    }

    /// Adds a persisted model to the pool used when resolving factory owners.
    #[must_use]
    pub fn recycle<R: Model>(self, model: R) -> Self {
        self.recycleMany([model])
    }

    /// Adds several persisted models of one type to the recycle pool.
    #[must_use]
    pub fn recycleMany<R: Model>(mut self, models: impl IntoIterator<Item = R>) -> Self {
        self.recycled.extend(models.into_iter().filter_map(|model| {
            if !model.state().exists() {
                return None;
            }
            Some(RecycledModel {
                connection: model.state().connection.clone()?,
                model: Arc::new(model),
            })
        }));
        self
    }

    fn recycle_pool(&self, inherited: &[RecycledModel]) -> Vec<RecycledModel> {
        inherited.iter().chain(&self.recycled).cloned().collect()
    }

    #[must_use]
    pub fn state(
        mut self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.states
            .push(Box::new(move |attributes, _| state(attributes)));
        self
    }

    #[must_use]
    pub fn sequence(
        mut self,
        sequence: impl Fn(usize) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.states.push(Box::new(move |_, index| sequence(index)));
        self
    }

    pub fn count(self, count: usize) -> FactoryBatch<M> {
        FactoryBatch {
            factory: self,
            count,
        }
    }

    #[must_use]
    pub fn afterMaking(
        mut self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.after_making.push(Box::new(callback));
        self
    }

    #[must_use]
    pub fn afterCreating(
        mut self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.after_creating.push(Box::new(callback));
        self
    }

    #[must_use]
    pub fn has<R: Model>(
        mut self,
        relation: impl FactoryChild<M, R>,
        children: FactoryBatch<R>,
    ) -> Self {
        let relation = Arc::new((relation, children));
        self.children
            .push(Box::new(move |connection, parent, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, children) = &*relation;
                    let connection = related_connection::<R>(connection)?;
                    let pool = children.factory.recycle_pool(inherited);
                    let attributes = relation.attributes(parent)?;
                    let mut models = children
                        .make_with_pool(&connection, &pool, Some(&attributes))
                        .await?;
                    for child in models.models_mut() {
                        if !relation.save(parent, child).await? {
                            return Err(DatabaseError::InvalidQuery(
                                "factory child creation cancelled",
                            ));
                        }
                        children
                            .factory
                            .create_children(&connection, child, &pool)
                            .await?;
                    }
                    children
                        .factory
                        .run_callbacks(models.models_mut(), &children.factory.after_creating)?;
                    Ok(())
                })
            }));
        self
    }

    #[must_use]
    pub fn r#for<R: Model>(
        mut self,
        relation: impl FactoryParent<M, R>,
        owner: Factory<R>,
    ) -> Self {
        let relation = Arc::new((relation, owner));
        self.parents
            .push(Box::new(move |connection, model, cache, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, owner) = &*relation;
                    let owner = cache
                        .get_or_try_init(|| async {
                            let connection = related_connection::<R>(connection)?;
                            let pool = owner.recycle_pool(inherited);
                            if let Some(model) = recycled_owner::<R>(&pool, &connection) {
                                return Ok(model);
                            }
                            let model = owner
                                .create_one(&connection, 0, &owner.parent_cache(), &pool)
                                .await?;
                            Ok::<SharedModel, DatabaseError>(Arc::new(model))
                        })
                        .await?;
                    let owner = owner.downcast_ref::<R>().expect("factory owner cache type");
                    relation.associate(model, owner)
                })
            }));
        self
    }

    #[must_use]
    pub fn forExisting<R: Model>(mut self, relation: impl FactoryParent<M, R>, owner: R) -> Self {
        let relation = Arc::new((relation, owner));
        self.parents.push(Box::new(move |_, model, _, _| {
            let relation = Arc::clone(&relation);
            Box::pin(async move { relation.0.associate(model, &relation.1) })
        }));
        self
    }

    #[must_use]
    pub fn hasAttached<R: Model>(
        mut self,
        relation: impl Into<BelongsToMany<M, R>>,
        models: FactoryBatch<R>,
        pivot: Attributes,
    ) -> Self {
        let relation = Arc::new((relation.into(), models, pivot));
        self.children
            .push(Box::new(move |connection, parent, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, models, pivot) = &*relation;
                    let connection = related_connection::<R>(connection)?;
                    let attributes = (relation.factory_defaults(&connection)?, Attributes::new());
                    let models = models
                        .create_with_pool(&connection, inherited, Some(&attributes))
                        .await?;
                    let keys = models
                        .iter()
                        .map(|model| {
                            model
                                .getAttribute(relation.related_key())
                                .filter(|key| !matches!(key, Value::Null))
                                .cloned()
                                .ok_or(DatabaseError::InvalidQuery("factory related primary key"))
                        })
                        .collect::<DatabaseResult<Vec<_>>>()?;
                    relation.attachWith(parent, &keys, pivot.clone()).await
                })
            }));
        self
    }

    pub async fn make(&self) -> DatabaseResult<M> {
        self.makeOn(&DB::connection(M::metadata().connection)?)
            .await
    }

    pub async fn makeOn(&self, connection: &Connection) -> DatabaseResult<M> {
        let mut model = self
            .instantiate(connection, 0, &self.parent_cache(), &self.recycled, None)
            .await?;
        self.run_callbacks(std::slice::from_mut(&mut model), &self.after_making)?;
        Ok(model)
    }

    pub async fn create(&self) -> DatabaseResult<M> {
        self.createOn(&DB::connection(M::metadata().connection)?)
            .await
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<M> {
        self.create_one(connection, 0, &self.parent_cache(), &self.recycled)
            .await
    }

    fn parent_cache(&self) -> Vec<ParentCache> {
        self.parents.iter().map(|_| OnceCell::new()).collect()
    }

    async fn instantiate(
        &self,
        connection: &Connection,
        index: usize,
        cache: &[ParentCache],
        pool: &[RecycledModel],
        relation: Option<&(Attributes, Attributes)>,
    ) -> DatabaseResult<M> {
        let mut attributes = (self.definition)(connection)?;
        if let Some((defaults, _)) = relation {
            attributes.extend(defaults.clone());
        }
        for state in &self.states {
            let changes = state(&attributes, index);
            attributes.extend(changes);
        }
        if let Some((_, keys)) = relation {
            attributes.extend(keys.clone());
        }
        let mut model = M::from_state(Default::default());
        model.state_mut().connection = Some(connection.clone());
        for (parent, cache) in self.parents.iter().zip(cache) {
            parent(connection, &mut model, cache, pool).await?;
        }
        // Parent keys provide defaults. Factory states and explicit overrides can replace them.
        let mut raw = model.state().attributes.clone();
        for (name, value) in attributes {
            casts::write_async::<M>(
                &mut raw,
                name,
                value,
                Some(connection),
                &model.state().casts,
            )
            .await?;
        }
        model.state_mut().replace_attributes(raw);
        Ok(model)
    }

    async fn create_one(
        &self,
        connection: &Connection,
        index: usize,
        cache: &[ParentCache],
        pool: &[RecycledModel],
    ) -> DatabaseResult<M> {
        let mut model = self
            .instantiate(connection, index, cache, pool, None)
            .await?;
        self.run_callbacks(std::slice::from_mut(&mut model), &self.after_making)?;
        self.store(connection, &mut model, pool).await?;
        self.run_callbacks(std::slice::from_mut(&mut model), &self.after_creating)?;
        Ok(model)
    }

    async fn store(
        &self,
        connection: &Connection,
        model: &mut M,
        pool: &[RecycledModel],
    ) -> DatabaseResult<()> {
        if !model.save().await? {
            return Err(DatabaseError::InvalidQuery(
                "factory model creation cancelled",
            ));
        }
        self.create_children(connection, model, pool).await
    }

    fn run_callbacks(
        &self,
        models: &mut [M],
        callbacks: &[Box<Callback<M>>],
    ) -> DatabaseResult<()> {
        for model in models {
            for callback in callbacks {
                callback(model)?;
            }
        }
        Ok(())
    }

    async fn create_children(
        &self,
        connection: &Connection,
        model: &mut M,
        pool: &[RecycledModel],
    ) -> DatabaseResult<()> {
        for child in &self.children {
            child(connection, model, pool).await?;
        }
        Ok(())
    }
}

fn related_connection<M: Model>(connection: &Connection) -> DatabaseResult<Connection> {
    match M::metadata().connection {
        Some(name) if name != connection.getName() => DB::connection(Some(name)),
        _ => Ok(connection.clone()),
    }
}

/// A factory that produces the requested number of models.
pub struct FactoryBatch<M: Model> {
    factory: Factory<M>,
    count: usize,
}

#[allow(non_snake_case, reason = "Matches Laravel's factory API")]
impl<M: Model> FactoryBatch<M> {
    #[must_use]
    pub fn recycle<R: Model>(self, model: R) -> Self {
        self.recycleMany([model])
    }

    #[must_use]
    pub fn recycleMany<R: Model>(self, models: impl IntoIterator<Item = R>) -> Self {
        Self {
            factory: self.factory.recycleMany(models),
            count: self.count,
        }
    }

    pub async fn make(&self) -> DatabaseResult<Collection<M>> {
        self.makeOn(&DB::connection(M::metadata().connection)?)
            .await
    }

    pub async fn makeOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        self.make_with_pool(connection, &self.factory.recycled, None)
            .await
    }

    async fn make_with_pool(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        relation: Option<&(Attributes, Attributes)>,
    ) -> DatabaseResult<Collection<M>> {
        let mut models = Vec::with_capacity(self.count);
        let cache = self.factory.parent_cache();
        for index in 0..self.count {
            models.push(
                self.factory
                    .instantiate(connection, index, &cache, pool, relation)
                    .await?,
            );
        }
        let mut models = M::newCollection(models);
        self.factory
            .run_callbacks(models.models_mut(), &self.factory.after_making)?;
        Ok(models)
    }

    pub async fn create(&self) -> DatabaseResult<Collection<M>> {
        self.createOn(&DB::connection(M::metadata().connection)?)
            .await
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        self.create_with_pool(connection, &[], None).await
    }

    async fn create_with_pool(
        &self,
        connection: &Connection,
        inherited: &[RecycledModel],
        relation: Option<&(Attributes, Attributes)>,
    ) -> DatabaseResult<Collection<M>> {
        let pool = self.factory.recycle_pool(inherited);
        let mut models = self.make_with_pool(connection, &pool, relation).await?;
        for model in models.models_mut() {
            self.factory.store(connection, model, &pool).await?;
        }
        self.factory
            .run_callbacks(models.models_mut(), &self.factory.after_creating)?;
        Ok(models)
    }
}
