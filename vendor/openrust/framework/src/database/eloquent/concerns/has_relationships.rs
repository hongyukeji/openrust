use std::{any::Any, collections::BTreeMap};

use indexmap::IndexMap;

use crate::database::{DatabaseError, DatabaseFuture, DatabaseResult};

use super::super::{Model, relations::Relation};

#[derive(Default)]
pub(in crate::database::eloquent) struct LoadedRelations {
    values: BTreeMap<String, Box<dyn Any + Send + Sync>>,
}

impl LoadedRelations {
    pub(in crate::database::eloquent) fn get<T: Send + Sync + 'static>(
        &self,
        name: &str,
    ) -> Option<&T> {
        self.values.get(name)?.downcast_ref()
    }

    pub(in crate::database::eloquent) fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    pub(in crate::database::eloquent) fn insert<T: Send + Sync + 'static>(
        &mut self,
        name: String,
        value: T,
    ) {
        self.values.insert(name, Box::new(value));
    }

    pub(in crate::database::eloquent) fn remove(&mut self, name: &str) {
        self.values.remove(name);
    }

    pub(in crate::database::eloquent) fn clear(&mut self) {
        self.values.clear();
    }
}

pub(in crate::database::eloquent) async fn load<M, R>(
    models: &mut [M],
    name: &str,
    relation: &R,
    only_missing: bool,
) -> DatabaseResult<()>
where
    M: Model,
    R: Relation<Parent = M>,
{
    let (indexes, parents): (Vec<_>, Vec<_>) = models
        .iter()
        .enumerate()
        .filter(|(_, model)| !only_missing || !model.relationLoaded(name))
        .unzip();
    if parents.is_empty() {
        return Ok(());
    }
    let values = relation.matchResults(&parents).await?;
    if indexes.len() != values.len() {
        return Err(DatabaseError::InvalidQuery("relation result cardinality"));
    }
    for (index, value) in indexes.into_iter().zip(values) {
        models[index]
            .state_mut()
            .relations
            .insert(name.to_owned(), value);
    }
    Ok(())
}

pub(in crate::database::eloquent) async fn load_on_access<M, R>(
    model: &mut M,
    name: &str,
    relation: &R,
) -> DatabaseResult<()>
where
    M: Model,
    R: Relation<Parent = M>,
{
    if model.preventsLazyLoading() {
        model.handleLazyLoadingViolationUsing(DatabaseError::LazyLoadingViolation {
            model: std::any::type_name::<M>(),
            relation: name.to_owned(),
        })?;
    }
    model.load(name, relation).await
}

pub(in crate::database::eloquent) trait EagerLoad<M: Model>:
    Send + Sync
{
    fn load<'a>(
        &'a self,
        name: &'a str,
        models: &'a mut [M],
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

impl<M, R> EagerLoad<M> for R
where
    M: Model,
    R: Relation<Parent = M>,
{
    fn load<'a>(
        &'a self,
        name: &'a str,
        models: &'a mut [M],
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(load(models, name, self, false))
    }
}

pub(in crate::database::eloquent) async fn load_eager<M: Model>(
    eager_load: EagerLoads<M>,
    models: &mut [M],
) -> DatabaseResult<()> {
    if models.is_empty() {
        return Ok(());
    }
    for (name, relation) in eager_load.relations {
        relation.load(&name, models).await?;
    }
    Ok(())
}

/// The typed relationships to eager load for a model query.
pub struct EagerLoads<M: Model> {
    relations: IndexMap<String, Box<dyn EagerLoad<M>>>,
}

impl<M: Model> Default for EagerLoads<M> {
    fn default() -> Self {
        Self {
            relations: IndexMap::new(),
        }
    }
}

impl<M: Model> EagerLoads<M> {
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, relation: impl Relation<Parent = M>) -> Self {
        self.relations.insert(name.into(), Box::new(relation));
        self
    }

    #[must_use]
    pub fn without(mut self, names: &[&str]) -> Self {
        for name in names {
            self.relations.shift_remove(*name);
        }
        self
    }
}
