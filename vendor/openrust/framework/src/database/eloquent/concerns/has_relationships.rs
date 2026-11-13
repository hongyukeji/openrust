use std::{any::Any, collections::BTreeMap, sync::Arc};

use indexmap::IndexMap;
use serde_json::{Map, Value as Json};

use crate::database::{DatabaseError, DatabaseFuture, DatabaseResult, Transaction};

use super::super::{Model, relations::Relation};

#[derive(Clone, Default)]
pub(in crate::database::eloquent) struct LoadedRelations {
    values: BTreeMap<String, Arc<dyn LoadedRelation>>,
}

trait LoadedRelation: Send + Sync {
    fn value(&self) -> &dyn Any;
    fn project(&self) -> DatabaseResult<Option<Json>>;
    fn queueable_paths(&self) -> Option<Vec<String>>;
    fn reloadable(&self) -> bool;
}

struct LoadedValue<T> {
    value: T,
    project: fn(&T) -> DatabaseResult<Option<Json>>,
    queueable_paths: Option<fn(&T) -> Vec<String>>,
}

impl<T: Send + Sync + 'static> LoadedRelation for LoadedValue<T> {
    fn value(&self) -> &dyn Any {
        &self.value
    }

    fn project(&self) -> DatabaseResult<Option<Json>> {
        (self.project)(&self.value)
    }

    fn queueable_paths(&self) -> Option<Vec<String>> {
        self.queueable_paths.map(|paths| paths(&self.value))
    }
    fn reloadable(&self) -> bool {
        self.queueable_paths.is_some()
    }
}

impl LoadedRelations {
    pub(in crate::database::eloquent) fn get<T: Send + Sync + 'static>(
        &self,
        name: &str,
    ) -> Option<&T> {
        self.values.get(name)?.value().downcast_ref()
    }

    pub(in crate::database::eloquent) fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    pub(in crate::database::eloquent) fn insert<T: Send + Sync + 'static>(
        &mut self,
        name: String,
        value: T,
        project: fn(&T) -> DatabaseResult<Option<Json>>,
    ) {
        self.values.insert(
            name,
            Arc::new(LoadedValue {
                value,
                project,
                queueable_paths: None,
            }),
        );
    }

    pub(in crate::database::eloquent) fn insert_relation<R: Relation>(
        &mut self,
        name: String,
        value: R::Output,
    ) {
        self.values.insert(
            name,
            Arc::new(LoadedValue {
                value,
                project: R::serialize,
                queueable_paths: Some(R::queueableRelations),
            }),
        );
    }

    pub(in crate::database::eloquent) fn reloadable_names(&self) -> Vec<String> {
        self.values
            .iter()
            .filter(|(_, relation)| relation.reloadable())
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub(in crate::database::eloquent) fn queueable_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for (name, relation) in &self.values {
            if let Some(children) = relation.queueable_paths() {
                paths.push(name.clone());
                paths.extend(children.into_iter().map(|child| format!("{name}.{child}")));
            }
        }
        paths
    }

    pub(in crate::database::eloquent) fn project(
        &self,
        include: impl Fn(&str) -> bool,
    ) -> DatabaseResult<Map<String, Json>> {
        let mut output = Map::new();
        for (name, relation) in self.values.iter().filter(|(name, _)| include(name)) {
            if let Some(value) = relation.project()? {
                output.insert(super::super::serialization::snake_relation(name), value);
            }
        }
        Ok(output)
    }

    pub(in crate::database::eloquent) fn only(&self, name: &str) -> Self {
        let mut output = Self::default();
        output.copy_from(name, self);
        output
    }

    pub(in crate::database::eloquent) fn same_value(&self, name: &str, other: &Self) -> bool {
        self.values
            .get(name)
            .zip(other.values.get(name))
            .is_some_and(|(left, right)| Arc::ptr_eq(left, right))
    }

    pub(in crate::database::eloquent) fn copy_from(&mut self, name: &str, other: &Self) {
        if let Some(value) = other.values.get(name) {
            self.values.insert(name.to_owned(), value.clone());
        }
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
    load_with_transaction(models, name, relation, only_missing, None).await
}

async fn load_with_transaction<M, R>(
    models: &mut [M],
    name: &str,
    relation: &R,
    only_missing: bool,
    transaction: Option<&mut Transaction>,
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
    let values = match transaction {
        Some(transaction) => {
            relation
                .matchResultsInTransaction(&parents, transaction)
                .await?
        }
        None => relation.matchResults(&parents).await?,
    };
    if indexes.len() != values.len() {
        return Err(DatabaseError::InvalidQuery("relation result cardinality"));
    }
    for (index, value) in indexes.into_iter().zip(values) {
        models[index]
            .state_mut()
            .relations
            .insert_relation::<R>(name.to_owned(), value);
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
        transaction: Option<&'a mut Transaction>,
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
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(load_with_transaction(
            models,
            name,
            self,
            false,
            transaction,
        ))
    }
}

pub(in crate::database::eloquent) async fn load_eager<M: Model>(
    eager_load: EagerLoads<M>,
    models: &mut [M],
) -> DatabaseResult<()> {
    load_eager_with_transaction(eager_load, models, None).await
}

pub(in crate::database::eloquent) async fn load_eager_with_transaction<M: Model>(
    eager_load: EagerLoads<M>,
    models: &mut [M],
    mut transaction: Option<&mut Transaction>,
) -> DatabaseResult<()> {
    if models.is_empty() {
        return Ok(());
    }
    for (name, relation) in eager_load.relations {
        relation
            .load(&name, models, transaction.as_deref_mut())
            .await?;
    }
    Ok(())
}

/// The typed relationships to eager load for a model query.
pub struct EagerLoads<M: Model> {
    relations: IndexMap<String, Arc<dyn EagerLoad<M>>>,
}

impl<M: Model> Clone for EagerLoads<M> {
    fn clone(&self) -> Self {
        Self {
            relations: self.relations.clone(),
        }
    }
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
        self.relations.insert(name.into(), Arc::new(relation));
        self
    }

    pub(in crate::database::eloquent) fn extend(&mut self, other: Self) {
        self.relations.extend(other.relations);
    }

    #[must_use]
    pub fn without(mut self, names: &[&str]) -> Self {
        for name in names {
            self.relations.shift_remove(*name);
        }
        self
    }
}
