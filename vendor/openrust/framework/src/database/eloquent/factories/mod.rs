//! Eloquent model factories for seeders and tests.

use std::sync::Arc;

use crate::database::{Connection, DB, DatabaseError, DatabaseFuture, DatabaseResult, Value};

use super::{
    Attributes, Collection, Model, casts,
    relations::{BelongsTo, BelongsToMany, HasMany},
};

type Definition = dyn Fn() -> Attributes + Send + Sync;
type State = dyn Fn(&Attributes, usize) -> Attributes + Send + Sync;
type Callback<M> = dyn Fn(&mut M) -> DatabaseResult<()> + Send + Sync;
type Relationship<M> = dyn for<'a> Fn(&'a Connection, &'a mut M) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;

/// Associates a model with the factory definition chosen by its application.
pub trait HasFactory: Model {
    fn factory() -> Factory<Self>;
}

/// A model definition with ordered attribute states and relationship callbacks.
pub struct Factory<M: Model> {
    definition: Box<Definition>,
    states: Vec<Box<State>>,
    parents: Vec<Box<Relationship<M>>>,
    children: Vec<Box<Relationship<M>>>,
    after_making: Vec<Box<Callback<M>>>,
    after_creating: Vec<Box<Callback<M>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's factory API")]
impl<M: Model> Factory<M> {
    pub fn new(definition: impl Fn() -> Attributes + Send + Sync + 'static) -> Self {
        Self {
            definition: Box::new(definition),
            states: Vec::new(),
            parents: Vec::new(),
            children: Vec::new(),
            after_making: Vec::new(),
            after_creating: Vec::new(),
        }
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
    pub fn has<R: Model>(mut self, relation: HasMany<M, R>, children: FactoryBatch<R>) -> Self {
        let relation = Arc::new((relation, children));
        self.children.push(Box::new(move |connection, parent| {
            let relation = Arc::clone(&relation);
            Box::pin(async move {
                let (relation, children) = &*relation;
                let connection = related_connection::<R>(connection)?;
                for index in 0..children.count {
                    let mut child = children.factory.make_one(&connection, index).await?;
                    relation.save(parent, &mut child).await?;
                    children.factory.complete(&connection, &mut child).await?;
                }
                Ok(())
            })
        }));
        self
    }

    #[must_use]
    pub fn r#for<R: Model>(mut self, relation: BelongsTo<M, R>, owner: Factory<R>) -> Self {
        let relation = Arc::new((relation, owner));
        self.parents.push(Box::new(move |connection, model| {
            let relation = Arc::clone(&relation);
            Box::pin(async move {
                let (relation, owner) = &*relation;
                let connection = related_connection::<R>(connection)?;
                let owner = owner.createOn(&connection).await?;
                relation.associate(model, &owner)
            })
        }));
        self
    }

    #[must_use]
    pub fn forExisting<R: Model>(mut self, relation: BelongsTo<M, R>, owner: R) -> Self {
        let relation = Arc::new((relation, owner));
        self.parents.push(Box::new(move |_, model| {
            let relation = Arc::clone(&relation);
            Box::pin(async move { relation.0.associate(model, &relation.1) })
        }));
        self
    }

    #[must_use]
    pub fn hasAttached<R: Model>(
        mut self,
        relation: BelongsToMany<M, R>,
        models: FactoryBatch<R>,
        pivot: Attributes,
    ) -> Self {
        let relation = Arc::new((relation, models, pivot));
        self.children.push(Box::new(move |connection, parent| {
            let relation = Arc::clone(&relation);
            Box::pin(async move {
                let (relation, models, pivot) = &*relation;
                let connection = related_connection::<R>(connection)?;
                let models = models.createOn(&connection).await?;
                let keys = models
                    .iter()
                    .map(|model| {
                        model
                            .getKey()
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
        self.make_one(connection, 0).await
    }

    pub async fn create(&self) -> DatabaseResult<M> {
        self.createOn(&DB::connection(M::metadata().connection)?)
            .await
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<M> {
        self.create_one(connection, 0).await
    }

    async fn make_one(&self, connection: &Connection, index: usize) -> DatabaseResult<M> {
        let mut attributes = (self.definition)();
        for state in &self.states {
            let changes = state(&attributes, index);
            attributes.extend(changes);
        }
        let mut model = M::from_state(Default::default());
        model.state_mut().connection = Some(connection.clone());
        for parent in &self.parents {
            parent(connection, &mut model).await?;
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
        for callback in &self.after_making {
            callback(&mut model)?;
        }
        Ok(model)
    }

    async fn create_one(&self, connection: &Connection, index: usize) -> DatabaseResult<M> {
        let mut model = self.make_one(connection, index).await?;
        model.save().await?;
        self.complete(connection, &mut model).await?;
        Ok(model)
    }

    async fn complete(&self, connection: &Connection, model: &mut M) -> DatabaseResult<()> {
        for child in &self.children {
            child(connection, model).await?;
        }
        for callback in &self.after_creating {
            callback(model)?;
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
    pub async fn make(&self) -> DatabaseResult<Collection<M>> {
        self.makeOn(&DB::connection(M::metadata().connection)?)
            .await
    }

    pub async fn makeOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        let mut models = Vec::new();
        for index in 0..self.count {
            models.push(self.factory.make_one(connection, index).await?);
        }
        Ok(Collection::new(models))
    }

    pub async fn create(&self) -> DatabaseResult<Collection<M>> {
        self.createOn(&DB::connection(M::metadata().connection)?)
            .await
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        let mut models = Vec::new();
        for index in 0..self.count {
            models.push(self.factory.create_one(connection, index).await?);
        }
        Ok(Collection::new(models))
    }
}
