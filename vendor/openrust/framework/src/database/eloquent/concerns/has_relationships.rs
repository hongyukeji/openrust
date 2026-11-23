use std::{any::Any, sync::Arc};

use indexmap::IndexMap;
use serde_json::{Map, Value as Json};

use crate::database::{DatabaseError, DatabaseFuture, DatabaseResult, Transaction};

use super::super::{
    Model,
    relations::{Pivot, PushContext, Relation},
};

#[derive(Clone, Default)]
pub(in crate::database::eloquent) struct LoadedRelations {
    values: IndexMap<String, Arc<dyn LoadedRelation>>,
}

type PushedRelation<'a> =
    DatabaseFuture<'a, (Option<Arc<dyn LoadedRelation>>, DatabaseResult<bool>)>;
type PushOutput<T> =
    for<'a> fn(&'a T, &'a mut PushContext) -> DatabaseFuture<'a, (Option<T>, DatabaseResult<bool>)>;

trait LoadedRelation: Send + Sync {
    fn value(&self) -> &dyn Any;
    fn project(&self) -> DatabaseResult<Option<Json>>;
    fn queueable_paths(&self) -> Option<Vec<String>>;
    fn reloadable(&self) -> bool;
    fn is_empty_collection(&self) -> bool;
    fn pivot_table(&self) -> Option<&'static str>;
    fn push<'a>(&'a self, context: &'a mut PushContext) -> PushedRelation<'a>;
}

struct LoadedValue<T> {
    value: T,
    project: fn(&T) -> DatabaseResult<Option<Json>>,
    queueable_paths: Option<fn(&T) -> Vec<String>>,
    push: Option<PushOutput<T>>,
    is_empty_collection: Option<fn(&T) -> bool>,
    pivot_table: Option<fn(&T) -> &'static str>,
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
    fn is_empty_collection(&self) -> bool {
        self.is_empty_collection
            .is_some_and(|check| check(&self.value))
    }

    fn pivot_table(&self) -> Option<&'static str> {
        self.pivot_table.map(|table| table(&self.value))
    }

    fn push<'a>(&'a self, context: &'a mut PushContext) -> PushedRelation<'a> {
        Box::pin(async move {
            let Some(push) = self.push else {
                return (None, Ok(true));
            };
            let (value, result) = push(&self.value, context).await;
            let replacement = value.map(|value| {
                Arc::new(Self {
                    value,
                    project: self.project,
                    queueable_paths: self.queueable_paths,
                    push: self.push,
                    is_empty_collection: self.is_empty_collection,
                    pivot_table: self.pivot_table,
                }) as Arc<dyn LoadedRelation>
            });
            (replacement, result)
        })
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

    pub(in crate::database::eloquent) fn pivot_table(&self, name: &str) -> Option<&'static str> {
        self.values.get(name)?.pivot_table()
    }

    pub(in crate::database::eloquent) fn insert_pivot(&mut self, name: String, value: Pivot) {
        self.values.insert(
            name,
            Arc::new(LoadedValue {
                value,
                project: Pivot::serialize,
                queueable_paths: None,
                push: None,
                is_empty_collection: None,
                pivot_table: Some(Pivot::getTable),
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
                push: Some(R::pushLoaded),
                is_empty_collection: Some(R::isEmptyCollection),
                pivot_table: None,
            }),
        );
    }

    pub(in crate::database::eloquent) fn insert_inverse<M: Model>(
        &mut self,
        name: String,
        model: Option<Arc<M>>,
    ) {
        self.values.insert(
            name,
            Arc::new(LoadedValue {
                value: model,
                project: super::super::serialization::one::<M>,
                queueable_paths: None,
                is_empty_collection: None,
                pivot_table: None,
                push: Some(|model, context| Box::pin(context.push_one(model))),
            }),
        );
    }

    pub(in crate::database::eloquent) fn insert_pivot_model<M: Model>(
        &mut self,
        name: String,
        model: M,
        project: fn(&M) -> DatabaseResult<Option<Json>>,
    ) {
        self.values.insert(
            name,
            Arc::new(LoadedValue {
                value: model,
                project,
                queueable_paths: None,
                is_empty_collection: None,
                pivot_table: Some(M::getTable),
                push: Some(|model, context| {
                    Box::pin(async move {
                        let mut model = M::from_state(model.state().clone());
                        let result = context.push(&mut model).await;
                        (Some(model), result)
                    })
                }),
            }),
        );
    }

    pub(in crate::database::eloquent) async fn push(
        &mut self,
        context: &mut PushContext,
    ) -> DatabaseResult<bool> {
        for value in self.values.values_mut() {
            let (replacement, result) = value.push(context).await;
            if let Some(replacement) = replacement {
                *value = replacement;
            }
            if !result? {
                return Ok(false);
            }
        }
        Ok(true)
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
        self.values.shift_remove(name);
    }

    pub(in crate::database::eloquent) fn remove_empty_collections(&mut self) {
        self.values
            .retain(|_, relation| !relation.is_empty_collection());
    }

    pub(in crate::database::eloquent) fn clear(&mut self) {
        self.values.clear();
    }
}

// Run before editing the foreign key so detached autoload snapshots still match.
pub(in crate::database::eloquent) fn update_association_cache<R: Relation>(
    model: &mut R::Parent,
    name: Option<&str>,
    output: impl FnOnce() -> Option<R::Output>,
) {
    let state = model.state_mut();
    if let Some(name) = name {
        if let Some(link) = state.detached_autoload.take() {
            state.relations = link.visible_relations(state, |key| key != name);
        }
        match output() {
            Some(output) => state
                .relations
                .insert_relation::<R>(name.to_owned(), output),
            None => state.relations.remove(name),
        }
    } else {
        state.relations.clear();
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
        .filter(|(_, model)| !only_missing || !model.relation_loaded(name))
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
    let output = relation.getResults(model).await?;
    model
        .state_mut()
        .relations
        .insert_relation::<R>(name.to_owned(), output);
    Ok(())
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
    /// 按注册顺序读取预加载关系名称。
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.relations.keys().map(String::as_str)
    }

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
