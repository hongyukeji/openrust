//! Eloquent model factories for seeders and tests.

use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use tokio::sync::OnceCell;

use crate::{
    contracts::container::Container as _,
    database::{
        Connection, DB, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
    },
};

use super::{
    Attributes, Collection, Model, casts,
    relations::{BelongsToMany, Relation},
};

type Definition =
    dyn Fn(&Connection, &std::sync::OnceLock<Faker>) -> DatabaseResult<Attributes> + Send + Sync;
type State = dyn Fn(&Attributes, usize) -> Attributes + Send + Sync;
type Callback<M> = dyn Fn(&mut M) -> DatabaseResult<()> + Send + Sync;
type PivotResolver<M> = dyn Fn(&M) -> DatabaseResult<Attributes> + Send + Sync;
type Relationship<M> = dyn for<'a> Fn(
        &'a Connection,
        &'a mut M,
        &'a [RecycledModel],
    ) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;

mod faker;
mod regex_pattern;
mod relationships;

pub use faker::{Faker, UniqueFaker, fake};

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

type ParentAttributeResolver = dyn for<'a> Fn(&'a Connection, &'a [RecycledModel]) -> DatabaseFuture<'a, DatabaseResult<Value>>
    + Send
    + Sync;

struct ParentAttribute {
    name: String,
    model: TypeId,
    resolve: Box<ParentAttributeResolver>,
}

/// Identifies a factory-valued parent attribute to exclude from expansion.
pub enum FactoryParentSelector {
    Attribute(String),
    Model(TypeId),
}

impl FactoryParentSelector {
    pub fn attribute(name: impl Into<String>) -> Self {
        Self::Attribute(name.into())
    }

    pub fn model<R: Model>() -> Self {
        Self::Model(TypeId::of::<R>())
    }
}

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

/// Selects generated or already persisted models for a factory attachment.
pub enum FactoryAttachment<R: Model> {
    Generated(Box<FactoryBatch<R>>),
    Existing(Vec<R>),
}

impl<R: Model> From<FactoryBatch<R>> for FactoryAttachment<R> {
    fn from(factory: FactoryBatch<R>) -> Self {
        Self::Generated(Box::new(factory))
    }
}

impl<R: Model> From<R> for FactoryAttachment<R> {
    fn from(model: R) -> Self {
        Self::Existing(vec![model])
    }
}

impl<R: Model> From<Vec<R>> for FactoryAttachment<R> {
    fn from(models: Vec<R>) -> Self {
        Self::Existing(models)
    }
}

impl<R: Model> From<Collection<R>> for FactoryAttachment<R> {
    fn from(models: Collection<R>) -> Self {
        Self::Existing(models.into_iter().collect())
    }
}

/// Supplies pivot attributes to each model created by an attached factory.
pub enum FactoryPivot<M: Model> {
    Shared(Attributes),
    Sequence(Vec<Attributes>),
    Resolve(Box<PivotResolver<M>>),
}

impl<M: Model> FactoryPivot<M> {
    pub fn resolve(
        callback: impl Fn(&M) -> DatabaseResult<Attributes> + Send + Sync + 'static,
    ) -> Self {
        Self::Resolve(Box::new(callback))
    }
}

impl<M: Model> From<Attributes> for FactoryPivot<M> {
    fn from(attributes: Attributes) -> Self {
        Self::Shared(attributes)
    }
}

impl<M: Model> From<Vec<Attributes>> for FactoryPivot<M> {
    fn from(attributes: Vec<Attributes>) -> Self {
        Self::Sequence(attributes)
    }
}

/// Selects the records to create with an Eloquent factory.
#[derive(Default)]
pub enum FactoryRecords {
    #[default]
    Default,
    Count(usize),
    Attributes(Vec<Attributes>),
}

impl FactoryRecords {
    fn into_attributes(self, default_count: usize) -> impl Iterator<Item = Attributes> {
        let (count, attributes) = match self {
            Self::Default => (default_count, Vec::new()),
            Self::Count(count) => (count, Vec::new()),
            Self::Attributes(attributes) => (0, attributes),
        };
        attributes
            .into_iter()
            .chain(std::iter::repeat_with(Attributes::new).take(count))
    }
}

/// A model definition with ordered attribute states and relationship callbacks.
pub struct Factory<M: Model> {
    definition: Box<Definition>,
    connection: Option<String>,
    states: Vec<Box<State>>,
    recycled: Vec<RecycledModel>,
    parents: Vec<Box<Parent<M>>>,
    parent_attributes: Vec<ParentAttribute>,
    excluded_parents: Option<Vec<FactoryParentSelector>>,
    children: Vec<Box<Relationship<M>>>,
    after_making: Vec<Box<Callback<M>>>,
    after_creating: Vec<Box<Callback<M>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's factory API")]
impl<M: Model> Factory<M> {
    pub fn new(definition: impl Fn() -> Attributes + Send + Sync + 'static) -> Self {
        Self::from_definition(move |_, _| Ok(definition()))
    }

    /// Defines model attributes using the application's configured Faker locale.
    pub fn withFaker(definition: impl Fn(&Faker) -> Attributes + Send + Sync + 'static) -> Self {
        Self::tryWithFaker(move |faker| Ok(definition(faker)))
    }

    /// Defines model attributes with a generator that can return an error.
    pub fn tryWithFaker(
        definition: impl Fn(&Faker) -> DatabaseResult<Attributes> + Send + Sync + 'static,
    ) -> Self {
        Self::from_definition(move |connection, faker| {
            if faker.get().is_none() {
                let _ = faker.set(Faker::for_connection(connection)?);
            }
            definition(faker.get().expect("factory generator initialized"))
        })
    }

    fn from_definition(
        definition: impl Fn(&Connection, &std::sync::OnceLock<Faker>) -> DatabaseResult<Attributes>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            definition: Box::new(definition),
            connection: None,
            states: Vec::new(),
            recycled: Vec::new(),
            parents: Vec::new(),
            parent_attributes: Vec::new(),
            excluded_parents: None,
            children: Vec::new(),
            after_making: Vec::new(),
            after_creating: Vec::new(),
        }
    }

    /// Selects the connection used by operations without an explicit connection argument.
    #[must_use]
    pub fn connection(mut self, name: Option<&str>) -> Self {
        self.connection = name.map(str::to_owned);
        self
    }

    pub fn getConnectionName(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    fn resolve_connection(&self) -> DatabaseResult<Connection> {
        DB::connection(self.getConnectionName().or(M::metadata().connection))
    }

    fn resolve_related_connection(&self, inherited: &Connection) -> DatabaseResult<Connection> {
        let name = self.getConnectionName().or(M::metadata().connection);
        let Some(name) = name.filter(|name| *name != inherited.getName()) else {
            return Ok(inherited.clone());
        };
        match Connection::model_container(Some(inherited)) {
            Some(container) => container
                .get::<DatabaseManager>("db")
                .map_err(DatabaseError::Unavailable)?
                .connection(Some(name)),
            None => DB::connection(Some(name)),
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

    /// Runs this transformation before the factory's existing states.
    #[must_use]
    pub fn prependState(
        mut self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.states
            .insert(0, Box::new(move |attributes, _| state(attributes)));
        self
    }

    /// Appends a state that sets one attribute.
    #[must_use]
    pub fn set(self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        let name = name.into();
        let value = value.into();
        self.state(move |_| Attributes::from([(name.clone(), value.clone())]))
    }

    #[must_use]
    pub fn sequence(
        mut self,
        sequence: impl Fn(usize) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.states.push(Box::new(move |_, index| sequence(index)));
        self
    }

    /// Sets a sequence of attribute maps and makes the batch as long as the sequence.
    pub fn forEachSequence(self, states: Vec<Attributes>) -> FactoryBatch<M> {
        let count = states.len();
        self.sequence(move |index| {
            index
                .checked_rem(count)
                .map(|offset| states[offset].clone())
                .unwrap_or_default()
        })
        .count(count)
    }

    /// Cycles through combinations in declaration order without allocating the product.
    pub fn crossJoinSequence(self, sequences: Vec<Vec<Attributes>>) -> DatabaseResult<Self> {
        if sequences.iter().any(Vec::is_empty) {
            return Err(DatabaseError::InvalidQuery(
                "factory cross join sequence is empty",
            ));
        }
        Ok(self.sequence(move |mut index| {
            let selected = sequences.iter().rev().map(|sequence| {
                let state = &sequence[index % sequence.len()];
                index /= sequence.len();
                state
            });
            let mut attributes = Attributes::new();
            for state in selected {
                for (name, value) in state {
                    attributes
                        .entry(name.clone())
                        .or_insert_with(|| value.clone());
                }
            }
            attributes
        }))
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

    /// Removes the current construction callbacks from this factory.
    #[must_use]
    pub fn withoutAfterMaking(mut self) -> Self {
        self.after_making.clear();
        self
    }

    /// Removes the current post-creation callbacks from this factory.
    #[must_use]
    pub fn withoutAfterCreating(mut self) -> Self {
        self.after_creating.clear();
        self
    }

    #[must_use]
    pub fn has<R: Model>(
        self,
        relation: impl FactoryChild<M, R>,
        children: FactoryBatch<R>,
    ) -> Self {
        let relation = Arc::new(relation);
        self.has_resolved(move |_| Ok(relation.clone()), children)
    }

    /// Resolves a registered model relationship by name and Rust descriptor type.
    #[must_use]
    pub fn hasNamed<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        name: impl Into<String>,
        children: FactoryBatch<R>,
    ) -> Self {
        let name = name.into();
        self.has_resolved(
            move |model| named_descriptor::<M, D>(model, &name).map(|relation| Arc::new(relation)),
            children,
        )
    }

    fn has_resolved<R: Model, D: FactoryChild<M, R>>(
        mut self,
        relation: impl Fn(&M) -> DatabaseResult<Arc<D>> + Send + Sync + 'static,
        children: FactoryBatch<R>,
    ) -> Self {
        let relation = Arc::new((relation, children));
        self.children
            .push(Box::new(move |connection, parent, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, children) = &*relation;
                    let relation = relation(parent)?;
                    let connection = children.factory.resolve_related_connection(connection)?;
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
    pub fn r#for<R: Model>(self, relation: impl FactoryParent<M, R>, owner: Factory<R>) -> Self {
        let relation = Arc::new(relation);
        self.for_resolved(move |_| Ok(relation.clone()), owner)
    }

    /// Resolves a registered model relationship by name and Rust descriptor type.
    #[must_use]
    pub fn forNamed<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        name: impl Into<String>,
        owner: Factory<R>,
    ) -> Self {
        let name = name.into();
        self.for_resolved(
            move |model| named_descriptor::<M, D>(model, &name).map(|relation| Arc::new(relation)),
            owner,
        )
    }

    fn for_resolved<R: Model, D: FactoryParent<M, R>>(
        mut self,
        relation: impl Fn(&M) -> DatabaseResult<Arc<D>> + Send + Sync + 'static,
        owner: Factory<R>,
    ) -> Self {
        let relation = Arc::new((relation, owner));
        self.parents
            .push(Box::new(move |connection, model, cache, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, owner) = &*relation;
                    let relation = relation(model)?;
                    let owner = cache
                        .get_or_try_init(|| async {
                            let connection = owner.resolve_related_connection(connection)?;
                            let pool = owner.recycle_pool(inherited);
                            if let Some(model) = recycled_owner::<R>(&pool, &connection) {
                                return Ok(model);
                            }
                            let model = owner
                                .create_one(
                                    &connection,
                                    0,
                                    &owner.parent_cache(),
                                    &pool,
                                    &std::sync::OnceLock::new(),
                                    Attributes::new(),
                                )
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

    /// Declares a factory-valued foreign key in the model definition.
    #[must_use]
    pub fn parentAttribute<R: Model>(mut self, name: impl Into<String>, owner: Factory<R>) -> Self {
        let name = name.into();
        self.parent_attributes
            .retain(|attribute| attribute.name != name);
        let owner = Arc::new(owner);
        self.parent_attributes.push(ParentAttribute {
            name,
            model: TypeId::of::<R>(),
            resolve: Box::new(move |connection, inherited| {
                let owner = Arc::clone(&owner);
                Box::pin(async move {
                    let connection = owner.resolve_related_connection(connection)?;
                    let pool = owner.recycle_pool(inherited);
                    let existing = recycled_owner::<R>(&pool, &connection);
                    let generated;
                    let related = if let Some(existing) = &existing {
                        existing.downcast_ref::<R>().expect("factory owner type")
                    } else {
                        generated = owner
                            .create_one(
                                &connection,
                                0,
                                &owner.parent_cache(),
                                &pool,
                                &std::sync::OnceLock::new(),
                                Attributes::new(),
                            )
                            .await?;
                        &generated
                    };
                    related
                        .getKey()
                        .filter(|key| !matches!(key, Value::Null))
                        .cloned()
                        .ok_or(DatabaseError::InvalidQuery("factory related primary key"))
                })
            }),
        });
        self
    }

    /// Excludes the selected factory-valued attributes, or all when the list is empty.
    #[must_use]
    pub fn withoutParents(mut self, parents: Vec<FactoryParentSelector>) -> Self {
        self.excluded_parents = Some(parents);
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
        self,
        relation: impl Into<BelongsToMany<M, R>>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        let relation = Arc::new(relation.into());
        self.has_attached_resolved(move |_| Ok(relation.clone()), models.into(), pivot.into())
    }

    /// Resolves a registered model relationship by name and Rust descriptor type.
    #[must_use]
    pub fn hasAttachedNamed<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        name: impl Into<String>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        let name = name.into();
        self.has_attached_resolved(
            move |model| {
                named_descriptor::<M, D>(model, &name).map(|relation| Arc::new(relation.into()))
            },
            models.into(),
            pivot.into(),
        )
    }

    fn has_attached_resolved<R: Model>(
        mut self,
        relation: impl Fn(&M) -> DatabaseResult<Arc<BelongsToMany<M, R>>> + Send + Sync + 'static,
        models: FactoryAttachment<R>,
        pivot: FactoryPivot<M>,
    ) -> Self {
        let relation = Arc::new((relation, models, pivot));
        self.children
            .push(Box::new(move |connection, parent, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, models, pivot) = &*relation;
                    let relation = relation(parent)?;
                    let generated;
                    let models: &[R] = match models {
                        FactoryAttachment::Generated(factory) => {
                            let connection =
                                factory.factory.resolve_related_connection(connection)?;
                            let attributes =
                                (relation.factory_defaults(&connection)?, Attributes::new());
                            generated = factory
                                .create_with_pool(&connection, inherited, Some(&attributes))
                                .await?;
                            &generated
                        }
                        FactoryAttachment::Existing(models) => {
                            relation.validate_factory_attachments(parent, models)?;
                            models
                        }
                    };
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
                    if let FactoryPivot::Shared(attributes) = pivot {
                        return relation.attachWith(parent, &keys, attributes.clone()).await;
                    }
                    for (index, key) in keys.iter().enumerate() {
                        let attributes = match pivot {
                            FactoryPivot::Shared(_) => unreachable!(),
                            FactoryPivot::Sequence(states) => index
                                .checked_rem(states.len())
                                .map(|offset| states[offset].clone())
                                .unwrap_or_default(),
                            FactoryPivot::Resolve(resolve) => resolve(parent)?,
                        };
                        relation
                            .attachWith(parent, std::slice::from_ref(key), attributes)
                            .await?;
                    }
                    Ok(())
                })
            }));
        self
    }

    pub async fn make(&self) -> DatabaseResult<M> {
        self.makeOn(&self.resolve_connection()?).await
    }

    pub async fn makeOn(&self, connection: &Connection) -> DatabaseResult<M> {
        self.makeOneOn(connection, Attributes::new()).await
    }

    pub async fn makeOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.makeOneOn(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn makeOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.make_one(
            connection,
            0,
            &self.parent_cache(),
            &self.recycled,
            &std::sync::OnceLock::new(),
            attributes,
        )
        .await
    }

    pub async fn makeMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.makeManyOn(&self.resolve_connection()?, records).await
    }

    pub async fn makeManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.make_many(connection, records, 1).await
    }

    async fn make_many(
        &self,
        connection: &Connection,
        records: FactoryRecords,
        default_count: usize,
    ) -> DatabaseResult<Collection<M>> {
        let cache = self.parent_cache();
        let faker = std::sync::OnceLock::new();
        let mut models = Vec::new();
        for (index, attributes) in records.into_attributes(default_count).enumerate() {
            models.push(
                self.make_one(
                    connection,
                    index,
                    &cache,
                    &self.recycled,
                    &faker,
                    attributes,
                )
                .await?,
            );
        }
        Ok(Collection::new(models))
    }

    /// Returns a reusable callback that creates the model when awaited.
    pub fn lazy<'a>(
        &'a self,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<M>> + 'a {
        move || Box::pin(self.createOne(attributes.clone()))
    }

    pub async fn create(&self) -> DatabaseResult<M> {
        self.createOn(&self.resolve_connection()?).await
    }

    /// Inserts the generated model without saving relations or firing model persistence events.
    pub async fn insert(&self, attributes: Attributes) -> DatabaseResult<()> {
        self.insertOn(&self.resolve_connection()?, attributes).await
    }

    pub async fn insertOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let model = self.makeOneOn(connection, attributes).await?;
        M::queryOn(connection)
            .withoutGlobalScopes()
            .insert_prepared_models(vec![model])
            .await
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<M> {
        self.createOneOn(connection, Attributes::new()).await
    }

    pub async fn createOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.createOneOn(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn createOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.create_one(
            connection,
            0,
            &self.parent_cache(),
            &self.recycled,
            &std::sync::OnceLock::new(),
            attributes,
        )
        .await
    }

    pub async fn createOneQuietly(&self, attributes: Attributes) -> DatabaseResult<M> {
        M::withoutEvents(self.createOne(attributes)).await
    }

    pub async fn createOneQuietlyOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        M::withoutEvents(self.createOneOn(connection, attributes)).await
    }

    pub async fn createMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.createManyOn(&self.resolve_connection()?, records)
            .await
    }

    pub async fn createManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.create_many(connection, records, 1).await
    }

    pub async fn createManyQuietly(
        &self,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createMany(records)).await
    }

    pub async fn createManyQuietlyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createManyOn(connection, records)).await
    }

    async fn create_many(
        &self,
        connection: &Connection,
        records: FactoryRecords,
        default_count: usize,
    ) -> DatabaseResult<Collection<M>> {
        let cache = self.parent_cache();
        let faker = std::sync::OnceLock::new();
        let mut models = Vec::new();
        for (index, attributes) in records.into_attributes(default_count).enumerate() {
            models.push(
                self.create_one(
                    connection,
                    index,
                    &cache,
                    &self.recycled,
                    &faker,
                    attributes,
                )
                .await?,
            );
        }
        Ok(Collection::new(models))
    }

    pub async fn createQuietly(&self) -> DatabaseResult<M> {
        M::withoutEvents(self.create()).await
    }

    pub async fn createQuietlyOn(&self, connection: &Connection) -> DatabaseResult<M> {
        M::withoutEvents(self.createOn(connection)).await
    }

    /// Expands the factory definition and parent keys without applying model casts or callbacks.
    pub async fn raw(&self) -> DatabaseResult<Attributes> {
        self.rawOn(&self.resolve_connection()?).await
    }

    pub async fn rawOn(&self, connection: &Connection) -> DatabaseResult<Attributes> {
        self.raw_attributes(
            connection,
            0,
            &self.parent_cache(),
            &std::sync::OnceLock::new(),
        )
        .await
    }

    async fn raw_attributes(
        &self,
        connection: &Connection,
        index: usize,
        cache: &[ParentCache],
        faker: &std::sync::OnceLock<Faker>,
    ) -> DatabaseResult<Attributes> {
        let (model, mut attributes) = self
            .expand_attributes(connection, index, cache, &self.recycled, None, faker)
            .await?;
        self.expand_parent_attributes(connection, &mut attributes, &self.recycled)
            .await?;
        let mut expanded = model.state().attributes.clone();
        expanded.extend(attributes);
        Ok(expanded)
    }

    fn parent_cache(&self) -> Vec<ParentCache> {
        self.parents.iter().map(|_| OnceCell::new()).collect()
    }

    async fn apply_attributes(
        connection: &Connection,
        mut model: M,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
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

    async fn expand_attributes(
        &self,
        connection: &Connection,
        index: usize,
        cache: &[ParentCache],
        pool: &[RecycledModel],
        relation: Option<&(Attributes, Attributes)>,
        faker: &std::sync::OnceLock<Faker>,
    ) -> DatabaseResult<(M, Attributes)> {
        let mut attributes = (self.definition)(connection, faker)?;
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
        Ok((model, attributes))
    }

    async fn expand_parent_attributes(
        &self,
        connection: &Connection,
        attributes: &mut Attributes,
        pool: &[RecycledModel],
    ) -> DatabaseResult<()> {
        for attribute in &self.parent_attributes {
            if attributes.contains_key(&attribute.name) {
                continue;
            }
            let excluded = self.excluded_parents.as_ref().is_some_and(|selectors| {
                selectors.is_empty()
                    || selectors.iter().any(|selector| match selector {
                        FactoryParentSelector::Attribute(name) => *name == attribute.name,
                        FactoryParentSelector::Model(model) => *model == attribute.model,
                    })
            });
            let value = if excluded {
                Value::Null
            } else {
                (attribute.resolve)(connection, pool).await?
            };
            attributes.insert(attribute.name.clone(), value);
        }
        Ok(())
    }

    async fn make_one(
        &self,
        connection: &Connection,
        index: usize,
        cache: &[ParentCache],
        pool: &[RecycledModel],
        faker: &std::sync::OnceLock<Faker>,
        overrides: Attributes,
    ) -> DatabaseResult<M> {
        let (model, mut attributes) = self
            .expand_attributes(connection, index, cache, pool, None, faker)
            .await?;
        attributes.extend(overrides);
        self.expand_parent_attributes(connection, &mut attributes, pool)
            .await?;
        let mut model = Self::apply_attributes(connection, model, attributes).await?;
        self.run_callbacks(std::slice::from_mut(&mut model), &self.after_making)?;
        Ok(model)
    }

    async fn create_one(
        &self,
        connection: &Connection,
        index: usize,
        cache: &[ParentCache],
        pool: &[RecycledModel],
        faker: &std::sync::OnceLock<Faker>,
        overrides: Attributes,
    ) -> DatabaseResult<M> {
        let mut model = self
            .make_one(connection, index, cache, pool, faker, overrides)
            .await?;
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

fn named_descriptor<M: Model, D: Relation<Parent = M>>(model: &M, name: &str) -> DatabaseResult<D> {
    match super::dynamic_relations::RelationResolvers::resolve(model, name)? {
        Some(descriptor) => Ok(descriptor),
        None => M::namedRelations().factory_descriptor(name),
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
    pub fn withoutParents(self, parents: Vec<FactoryParentSelector>) -> Self {
        self.factory.withoutParents(parents).count(self.count)
    }

    #[must_use]
    pub fn connection(self, name: Option<&str>) -> Self {
        self.factory.connection(name).count(self.count)
    }

    pub fn getConnectionName(&self) -> Option<&str> {
        self.factory.getConnectionName()
    }

    #[must_use]
    pub fn state(self, state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static) -> Self {
        self.factory.state(state).count(self.count)
    }

    #[must_use]
    pub fn prependState(
        self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.factory.prependState(state).count(self.count)
    }

    #[must_use]
    pub fn set(self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.factory.set(name, value).count(self.count)
    }

    #[must_use]
    pub fn withoutAfterMaking(self) -> Self {
        self.factory.withoutAfterMaking().count(self.count)
    }

    #[must_use]
    pub fn withoutAfterCreating(self) -> Self {
        self.factory.withoutAfterCreating().count(self.count)
    }

    pub async fn makeOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.makeOne(attributes).await
    }

    pub async fn makeOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory.makeOneOn(connection, attributes).await
    }

    pub async fn makeMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.makeManyOn(&self.factory.resolve_connection()?, records)
            .await
    }

    pub async fn makeManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.factory
            .make_many(connection, records, self.count)
            .await
    }

    /// Returns a reusable callback that creates the batch when awaited.
    pub fn lazy<'a>(
        &'a self,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<Collection<M>>> + 'a {
        move || {
            let attributes = attributes.clone();
            Box::pin(async move {
                let connection = self.factory.resolve_connection()?;
                let pool = self.factory.recycle_pool(&[]);
                let mut models = self
                    .make_with_overrides(&connection, &pool, None, &attributes)
                    .await?;
                self.store_models(&connection, &pool, &mut models).await?;
                Ok(models)
            })
        }
    }

    pub async fn createOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.createOne(attributes).await
    }

    pub async fn createOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory.createOneOn(connection, attributes).await
    }

    pub async fn createOneQuietly(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.createOneQuietly(attributes).await
    }

    pub async fn createOneQuietlyOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory
            .createOneQuietlyOn(connection, attributes)
            .await
    }

    pub async fn createMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.createManyOn(&self.factory.resolve_connection()?, records)
            .await
    }

    pub async fn createManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.factory
            .create_many(connection, records, self.count)
            .await
    }

    pub async fn createManyQuietly(
        &self,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createMany(records)).await
    }

    pub async fn createManyQuietlyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createManyOn(connection, records)).await
    }

    pub async fn raw(&self) -> DatabaseResult<Vec<Attributes>> {
        self.rawOn(&self.factory.resolve_connection()?).await
    }

    pub async fn rawOn(&self, connection: &Connection) -> DatabaseResult<Vec<Attributes>> {
        let cache = self.factory.parent_cache();
        let faker = std::sync::OnceLock::new();
        let mut records = Vec::with_capacity(self.count);
        for index in 0..self.count {
            records.push(
                self.factory
                    .raw_attributes(connection, index, &cache, &faker)
                    .await?,
            );
        }
        Ok(records)
    }

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
        self.makeOn(&self.factory.resolve_connection()?).await
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
        self.make_with_overrides(connection, pool, relation, &Attributes::new())
            .await
    }

    async fn make_with_overrides(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        relation: Option<&(Attributes, Attributes)>,
        overrides: &Attributes,
    ) -> DatabaseResult<Collection<M>> {
        let mut models = Vec::with_capacity(self.count);
        let cache = self.factory.parent_cache();
        let faker = std::sync::OnceLock::new();
        for index in 0..self.count {
            let (model, mut attributes) = self
                .factory
                .expand_attributes(connection, index, &cache, pool, relation, &faker)
                .await?;
            attributes.extend(overrides.clone());
            self.factory
                .expand_parent_attributes(connection, &mut attributes, pool)
                .await?;
            models.push(Factory::<M>::apply_attributes(connection, model, attributes).await?);
        }
        let mut models = M::newCollection(models);
        self.factory
            .run_callbacks(models.models_mut(), &self.factory.after_making)?;
        Ok(models)
    }

    pub async fn create(&self) -> DatabaseResult<Collection<M>> {
        self.createOn(&self.factory.resolve_connection()?).await
    }

    /// Inserts the generated batch in one statement after running afterMaking callbacks.
    pub async fn insert(&self, attributes: Attributes) -> DatabaseResult<()> {
        self.insertOn(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn insertOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let models = self
            .make_with_overrides(connection, &self.factory.recycled, None, &attributes)
            .await?;
        M::queryOn(connection)
            .withoutGlobalScopes()
            .insert_prepared_models(models.into_iter().collect())
            .await
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        self.create_with_pool(connection, &[], None).await
    }

    pub async fn createQuietly(&self) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.create()).await
    }

    pub async fn createQuietlyOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createOn(connection)).await
    }

    async fn create_with_pool(
        &self,
        connection: &Connection,
        inherited: &[RecycledModel],
        relation: Option<&(Attributes, Attributes)>,
    ) -> DatabaseResult<Collection<M>> {
        let pool = self.factory.recycle_pool(inherited);
        let mut models = self.make_with_pool(connection, &pool, relation).await?;
        self.store_models(connection, &pool, &mut models).await?;
        Ok(models)
    }

    async fn store_models(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        models: &mut Collection<M>,
    ) -> DatabaseResult<()> {
        for model in models.models_mut() {
            self.factory.store(connection, model, pool).await?;
        }
        self.factory
            .run_callbacks(models.models_mut(), &self.factory.after_creating)
    }
}
