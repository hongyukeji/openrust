use std::{collections::HashSet, ops::Deref, sync::Arc};

use crate::database::{DatabaseError, DatabaseResult, Value};

use crate::support::{ArrayKey, Collection as BaseCollection};

use super::{
    Attributes, Model, RelationAggregate, RelationAggregateQuery,
    collection::aggregate_attributes,
    concerns::has_relationships::LoadedRelations,
    relations::{ExistenceRelation, MorphTarget, MorphTo, Relation},
};

/// A relationship result that can be traversed as models.
pub trait RelatedModels {
    type Model: Model;

    fn models(&self) -> &[Arc<Self::Model>];

    #[doc(hidden)]
    fn replace_models(
        &self,
        _replace: impl FnMut(&Arc<Self::Model>) -> Arc<Self::Model>,
    ) -> DatabaseResult<Self>
    where
        Self: Sized,
    {
        Err(DatabaseError::InvalidQuery(
            "relationship output does not support nested loading",
        ))
    }
}

/// A keyed Eloquent relationship result backed by shared model snapshots.
pub struct RelatedCollection<M: Model> {
    models: Vec<Arc<M>>,
    pub(super) keys: Option<Vec<ArrayKey>>,
}

impl<M: Model> RelatedCollection<M> {
    /// Wraps shared model snapshots for explicit relation cache assignment.
    pub fn from_shared(models: Vec<Arc<M>>) -> Self {
        Self { models, keys: None }
    }

    pub(super) fn from_models(models: Vec<Arc<M>>) -> Self {
        let models = models
            .into_iter()
            .map(|model| {
                Arc::try_unwrap(model)
                    .unwrap_or_else(|shared| M::from_state(shared.state().clone()))
            })
            .collect();
        let (models, keys) = M::new_collection(models).into_shared_parts();
        Self { models, keys }
    }

    pub fn get_by_key(&self, key: impl Into<ArrayKey>) -> Option<&M> {
        let key = key.into().normalized();
        match (&self.keys, key) {
            (Some(keys), key) => keys
                .iter()
                .position(|candidate| *candidate == key)
                .map(|index| self.models[index].as_ref()),
            (None, ArrayKey::Integer(index)) => self
                .models
                .get(usize::try_from(index).ok()?)
                .map(Arc::as_ref),
            (None, ArrayKey::String(_)) => None,
        }
    }

    pub fn keys(&self) -> BaseCollection<ArrayKey> {
        match &self.keys {
            Some(keys) => BaseCollection::make(keys.iter().cloned()),
            None => BaseCollection::make((0..self.len()).map(|index| {
                ArrayKey::Integer(i64::try_from(index).expect("collection index fits i64"))
            })),
        }
    }

    pub(super) fn with_models(&self, models: Vec<Arc<M>>) -> Self {
        Self {
            models,
            keys: self.keys.clone(),
        }
    }
}

impl<M: Model> From<RelatedCollection<M>> for BaseCollection<Arc<M>> {
    fn from(group: RelatedCollection<M>) -> Self {
        match group.keys {
            Some(keys) => BaseCollection::from_entries(keys.into_iter().zip(group.models)),
            None => group.models.into_iter().collect(),
        }
    }
}

impl<M: Model> Clone for RelatedCollection<M> {
    fn clone(&self) -> Self {
        self.with_models(self.models.clone())
    }
}

impl<M: Model> Deref for RelatedCollection<M> {
    type Target = [Arc<M>];

    fn deref(&self) -> &Self::Target {
        &self.models
    }
}

impl<M: Model> IntoIterator for RelatedCollection<M> {
    type Item = Arc<M>;
    type IntoIter = std::vec::IntoIter<Arc<M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.models.into_iter()
    }
}

impl<'a, M: Model> IntoIterator for &'a RelatedCollection<M> {
    type Item = &'a Arc<M>;
    type IntoIter = std::slice::Iter<'a, Arc<M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.models.iter()
    }
}

impl<M: Model> RelatedModels for RelatedCollection<M> {
    type Model = M;

    fn models(&self) -> &[Arc<M>] {
        self
    }

    fn replace_models(&self, replace: impl FnMut(&Arc<M>) -> Arc<M>) -> DatabaseResult<Self> {
        Ok(self.with_models(self.iter().map(replace).collect()))
    }
}

impl<M: Model> RelatedModels for Vec<Arc<M>> {
    type Model = M;

    fn models(&self) -> &[Arc<M>] {
        self
    }

    fn replace_models(&self, replace: impl FnMut(&Arc<M>) -> Arc<M>) -> DatabaseResult<Self> {
        Ok(self.iter().map(replace).collect())
    }
}

impl<M: Model> RelatedModels for Option<Arc<M>> {
    type Model = M;

    fn models(&self) -> &[Arc<M>] {
        self.as_slice()
    }

    fn replace_models(&self, replace: impl FnMut(&Arc<M>) -> Arc<M>) -> DatabaseResult<Self> {
        Ok(self.as_ref().map(replace))
    }
}

/// Related models reached through one relationship across a parent collection.
pub struct RelatedBatch<'a, M: Model> {
    models: Vec<&'a M>,
    loaded: Vec<LoadedRelations>,
    aggregates: Vec<Attributes>,
}

impl<'a, M: Model> RelatedBatch<'a, M> {
    pub(super) fn new(models: impl Iterator<Item = &'a Arc<M>>) -> Self {
        Self::from_models(models.map(Arc::as_ref))
    }

    fn from_models(models: impl Iterator<Item = &'a M>) -> Self {
        let mut seen = HashSet::new();
        let models: Vec<_> = models
            .filter(|model| seen.insert(std::ptr::from_ref(*model)))
            .collect();
        let loaded = (0..models.len())
            .map(|_| LoadedRelations::default())
            .collect();
        Self {
            models,
            loaded,
            aggregates: Vec::new(),
        }
    }

    pub(super) fn from_morph_targets(
        targets: impl Iterator<Item = &'a MorphTarget>,
        alias: &str,
    ) -> DatabaseResult<Self> {
        let models = targets
            .filter(|target| target.alias() == alias)
            .map(|target| {
                target
                    .downcast_ref::<M>()
                    .ok_or(DatabaseError::InvalidQuery(
                        "morph type does not match model",
                    ))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        Ok(Self::from_models(models.into_iter()))
    }

    pub fn all(&self) -> &[&'a M] {
        &self.models
    }

    /// Reads an attribute with deferred aggregates applied to this batch.
    pub fn get_attribute(&self, index: usize, name: &str) -> Option<&Value> {
        self.aggregates
            .get(index)
            .and_then(|attributes| attributes.get(name))
            .or_else(|| self.models.get(index)?.get_attribute(name))
    }

    /// Loads a relation count for every model in this batch.
    pub async fn load_count(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<()> {
        let query = RelationAggregateQuery::new(name, relation, RelationAggregate::Count);
        let values = aggregate_attributes(&self.models, &[query]).await?;
        if self.aggregates.is_empty() {
            self.aggregates = values;
        } else {
            for (attributes, values) in self.aggregates.iter_mut().zip(values) {
                attributes.extend(values);
            }
        }
        Ok(())
    }

    /// Loads the relationship across the batch on the first missing access.
    pub async fn get_relation_or_load<R: Relation<Parent = M>>(
        &mut self,
        index: usize,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<&R::Output> {
        if index >= self.models.len() {
            return Err(DatabaseError::InvalidQuery("related model index"));
        }
        if !self.models[index].relation_loaded(name) && !self.loaded[index].contains(name) {
            self.load_missing(name, relation).await?;
        }
        self.result::<R>(index, name)
    }

    /// Continues the chain across every model in this batch.
    pub async fn related_models<R>(
        &mut self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<RelatedBatch<'_, <R::Output as RelatedModels>::Model>>
    where
        R: Relation<Parent = M>,
        R::Output: RelatedModels,
    {
        self.load_missing(name, relation).await?;
        let results = (0..self.models.len())
            .map(|index| self.result::<R>(index, name))
            .collect::<DatabaseResult<Vec<_>>>()?;
        Ok(RelatedBatch::new(
            results.into_iter().flat_map(RelatedModels::models),
        ))
    }

    /// Opens the selected polymorphic type as the next batch.
    pub async fn morph_models<T: Model>(
        &mut self,
        name: &str,
        relation: &MorphTo<M>,
        alias: &str,
    ) -> DatabaseResult<RelatedBatch<'_, T>> {
        relation.validate_target::<T>(alias)?;
        self.load_missing(name, relation).await?;
        let results = (0..self.models.len())
            .map(|index| self.result::<MorphTo<M>>(index, name))
            .collect::<DatabaseResult<Vec<_>>>()?;
        RelatedBatch::from_morph_targets(results.into_iter().filter_map(Option::as_ref), alias)
    }

    fn result<R: Relation<Parent = M>>(
        &self,
        index: usize,
        name: &str,
    ) -> DatabaseResult<&R::Output> {
        self.models[index]
            .get_relation(name)
            .or_else(|| self.loaded[index].get(name))
            .ok_or(DatabaseError::InvalidQuery("relationship output type"))
    }

    pub(super) async fn load_missing<R: Relation<Parent = M>>(
        &mut self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<()> {
        let (indexes, models): (Vec<_>, Vec<_>) = self
            .models
            .iter()
            .copied()
            .enumerate()
            .filter(|(index, model)| {
                !model.relation_loaded(name) && !self.loaded[*index].contains(name)
            })
            .unzip();
        if models.is_empty() {
            return Ok(());
        }
        let values = relation.match_results(&models).await?;
        if indexes.len() != values.len() {
            return Err(DatabaseError::InvalidQuery("relation result cardinality"));
        }
        for (index, value) in indexes.into_iter().zip(values) {
            self.loaded[index].insert_relation::<R>(name.to_owned(), value);
        }
        Ok(())
    }
}
