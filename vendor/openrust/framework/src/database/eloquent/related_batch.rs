use std::{collections::HashSet, sync::Arc};

use crate::database::{DatabaseError, DatabaseResult, Value};

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
}

impl<M: Model> RelatedModels for Vec<Arc<M>> {
    type Model = M;

    fn models(&self) -> &[Arc<M>] {
        self
    }
}

impl<M: Model> RelatedModels for Option<Arc<M>> {
    type Model = M;

    fn models(&self) -> &[Arc<M>] {
        self.as_slice()
    }
}

/// Related models reached through one relationship across a parent collection.
pub struct RelatedBatch<'a, M: Model> {
    models: Vec<&'a M>,
    loaded: Vec<LoadedRelations>,
    aggregates: Vec<Attributes>,
}

#[allow(non_snake_case, reason = "Matches Eloquent's relationship access API")]
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
    pub fn getAttribute(&self, index: usize, name: &str) -> Option<&Value> {
        self.aggregates
            .get(index)
            .and_then(|attributes| attributes.get(name))
            .or_else(|| self.models.get(index)?.getAttribute(name))
    }

    /// Loads a relation count for every model in this batch.
    pub async fn loadCount(
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
    pub async fn getRelationOrLoad<R: Relation<Parent = M>>(
        &mut self,
        index: usize,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<&R::Output> {
        if index >= self.models.len() {
            return Err(DatabaseError::InvalidQuery("related model index"));
        }
        if !self.models[index].relationLoaded(name) && !self.loaded[index].contains(name) {
            self.load_missing(name, relation).await?;
        }
        self.result::<R>(index, name)
    }

    /// Continues the chain across every model in this batch.
    pub async fn relatedModels<R>(
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
    pub async fn morphModels<T: Model>(
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
            .getRelation(name)
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
                !model.relationLoaded(name) && !self.loaded[*index].contains(name)
            })
            .unzip();
        if models.is_empty() {
            return Ok(());
        }
        let values = relation.matchResults(&models).await?;
        if indexes.len() != values.len() {
            return Err(DatabaseError::InvalidQuery("relation result cardinality"));
        }
        for (index, value) in indexes.into_iter().zip(values) {
            self.loaded[index].insert(name.to_owned(), value);
        }
        Ok(())
    }
}
