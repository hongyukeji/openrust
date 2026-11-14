use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, RwLock},
};

use crate::{
    contracts::container::Container as _,
    database::{Connection, DatabaseError, DatabaseFuture, DatabaseResult},
};

use super::{
    Attributes, Model,
    relations::{ExistenceRelation, Relation},
};

type Resolver<M, R> = dyn Fn(&RelationResolvers, &str, &M, &mut Vec<TypeId>) -> DatabaseResult<Option<R>>
    + Send
    + Sync;
type Registrations = HashMap<(TypeId, String), Resolvers>;

#[derive(Default)]
struct Resolvers {
    local: Option<Box<dyn Any + Send + Sync>>,
    inherited: Option<Box<dyn Any + Send + Sync>>,
}

#[derive(Default)]
pub(crate) struct RelationResolvers(RwLock<Registrations>);

impl RelationResolvers {
    pub(super) fn register<M: Model, R: Relation<Parent = M>>(
        &self,
        name: String,
        resolver: impl Fn(&M) -> R + Send + Sync + 'static,
    ) {
        let resolver: Arc<Resolver<M, R>> =
            Arc::new(move |_, _, model, _| Ok(Some(resolver(model))));
        self.0
            .write()
            .unwrap()
            .entry((TypeId::of::<M>(), name))
            .or_default()
            .local = Some(Box::new(resolver));
    }

    pub(super) fn inherit<M, B, S, R>(
        &self,
        name: String,
        base: impl Fn(&M) -> B + Send + Sync + 'static,
        adapt: impl Fn(S) -> R + Send + Sync + 'static,
    ) where
        M: Model,
        B: Model,
        S: Relation<Parent = B>,
        R: Relation<Parent = M>,
    {
        let resolver: Arc<Resolver<M, R>> = Arc::new(move |registry, name, model, path| {
            registry
                .lookup(&base(model), name, path)
                .map(|relation| relation.map(&adapt))
        });
        self.0
            .write()
            .unwrap()
            .entry((TypeId::of::<M>(), name))
            .or_default()
            .inherited = Some(Box::new(resolver));
    }

    pub(super) fn resolve<M: Model, R: Relation<Parent = M>>(
        model: &M,
        name: &str,
    ) -> DatabaseResult<Option<R>> {
        let Some(container) = Connection::model_container(model.state().connection.as_ref()) else {
            return Ok(None);
        };
        let registry = container
            .make::<Self>()
            .map_err(DatabaseError::Unavailable)?;
        registry.lookup(model, name, &mut Vec::new())
    }

    fn lookup<M: Model, R: Relation<Parent = M>>(
        &self,
        model: &M,
        name: &str,
        path: &mut Vec<TypeId>,
    ) -> DatabaseResult<Option<R>> {
        let model_type = TypeId::of::<M>();
        if path.contains(&model_type) {
            return Err(DatabaseError::InvalidQuery(
                "cyclic dynamic relationship inheritance",
            ));
        }
        let resolver = self
            .0
            .read()
            .unwrap()
            .get(&(model_type, name.to_owned()))
            .and_then(|entry| entry.local.as_ref().or(entry.inherited.as_ref()))
            .map(|resolver| {
                resolver
                    .downcast_ref::<Arc<Resolver<M, R>>>()
                    .cloned()
                    .ok_or(DatabaseError::InvalidQuery(
                        "dynamic relationship descriptor type",
                    ))
            })
            .transpose()?;
        let Some(resolver) = resolver else {
            return Ok(None);
        };
        path.push(model_type);
        let result = resolver(self, name, model, path);
        path.pop();
        result
    }
}

/// A named relationship that resolves its typed descriptor from the model's application.
pub struct DynamicRelation<R> {
    name: String,
    relation: PhantomData<fn() -> R>,
}

impl<R: Relation> DynamicRelation<R> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            relation: PhantomData,
        }
    }

    fn resolve(&self, parent: &R::Parent) -> DatabaseResult<R> {
        RelationResolvers::resolve(parent, &self.name)?
            .ok_or(DatabaseError::InvalidQuery("unknown dynamic relationship"))
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<R: Relation> Relation for DynamicRelation<R> {
    type Parent = R::Parent;
    type Output = R::Output;

    fn pushLoaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::relations::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        R::pushLoaded(output, context)
    }

    fn autoloadOutput(output: &Self::Output) -> Option<Self::Output> {
        R::autoloadOutput(output)
    }

    fn attachAutoload(outputs: &mut [Self::Output]) {
        R::attachAutoload(outputs);
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        R::serialize(output)
    }

    fn queueableRelations(output: &Self::Output) -> Vec<String> {
        R::queueableRelations(output)
    }

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a Self::Parent],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(async move {
            let descriptors = parents
                .iter()
                .map(|parent| self.resolve(parent))
                .collect::<DatabaseResult<Vec<_>>>()?;
            let mut results = Vec::with_capacity(parents.len());
            for (parent, descriptor) in parents.iter().zip(descriptors) {
                results.push(descriptor.getResults(parent).await?);
            }
            Ok(results)
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<R: ExistenceRelation> ExistenceRelation for DynamicRelation<R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<crate::database::query::Builder<'static>> {
        let prototype = R::Parent::newInstanceOn(connection, Attributes::new())?;
        self.resolve(&prototype)?
            .getRelationExistenceQuery(connection, parent_table)
    }
}
