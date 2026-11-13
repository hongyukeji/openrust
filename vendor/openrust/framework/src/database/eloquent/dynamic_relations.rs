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

use super::{Model, relations::Relation};

type Resolver<M, R> = dyn Fn(&M) -> R + Send + Sync;
type Registrations = HashMap<(TypeId, String), Box<dyn Any + Send + Sync>>;

#[derive(Default)]
pub(crate) struct RelationResolvers(RwLock<Registrations>);

impl RelationResolvers {
    pub(super) fn register<M: Model, R: Relation<Parent = M>>(
        &self,
        name: String,
        resolver: impl Fn(&M) -> R + Send + Sync + 'static,
    ) {
        let resolver: Arc<Resolver<M, R>> = Arc::new(resolver);
        self.0
            .write()
            .unwrap()
            .insert((TypeId::of::<M>(), name), Box::new(resolver));
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
        let resolver = registry
            .0
            .read()
            .unwrap()
            .get(&(TypeId::of::<M>(), name.to_owned()))
            .map(|resolver| {
                resolver
                    .downcast_ref::<Arc<Resolver<M, R>>>()
                    .cloned()
                    .ok_or(DatabaseError::InvalidQuery(
                        "dynamic relationship descriptor type",
                    ))
            })
            .transpose()?;
        Ok(resolver.map(|resolver| resolver(model)))
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
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<R: Relation> Relation for DynamicRelation<R> {
    type Parent = R::Parent;
    type Output = R::Output;

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        R::serialize(output)
    }

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a Self::Parent],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(async move {
            let descriptors = parents
                .iter()
                .map(|parent| {
                    RelationResolvers::resolve::<_, R>(*parent, &self.name)?
                        .ok_or(DatabaseError::InvalidQuery("unknown dynamic relationship"))
                })
                .collect::<DatabaseResult<Vec<_>>>()?;
            let mut results = Vec::with_capacity(parents.len());
            for (parent, descriptor) in parents.iter().zip(descriptors) {
                results.push(descriptor.getResults(parent).await?);
            }
            Ok(results)
        })
    }
}
