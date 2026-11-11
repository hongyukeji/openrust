use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use indexmap::IndexMap;

use crate::database::{Connection, DB, DatabaseError, DatabaseResult, Value};

use super::{
    Attributes, Builder, EagerLoads, Model, ModelState, RelatedBatch, RelatedModels,
    RelationAggregate, RelationAggregateQuery,
    concerns::has_relationships,
    relations::{ExistenceRelation, MorphTarget, MorphTo, Relation, relation::dictionary_key},
};

/// A collection of hydrated models, in query order.
pub struct Collection<M: Model> {
    models: Vec<M>,
    autoload: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent Collection API")]
impl<M: Model> Collection<M> {
    pub fn new(models: Vec<M>) -> Self {
        Self {
            models,
            autoload: M::automaticallyEagerLoadRelationships(),
        }
    }

    /// Batch-loads missing relationships when accessed through this collection.
    #[must_use]
    pub fn withRelationshipAutoloading(mut self) -> Self {
        self.autoload = true;
        self
    }

    /// Reads one model's relationship, loading the missing values on first access.
    pub async fn getRelationOrLoad<R: Relation<Parent = M>>(
        &mut self,
        index: usize,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<&R::Output> {
        let model = self
            .models
            .get(index)
            .ok_or(DatabaseError::InvalidQuery("model collection index"))?;
        if !model.relationLoaded(name) {
            if self.autoload {
                self.loadMissing(name, relation).await?;
            } else {
                has_relationships::load_on_access(&mut self.models[index], name, relation).await?;
            }
        }
        self.models[index]
            .getRelation(name)
            .ok_or(DatabaseError::InvalidQuery("relationship output type"))
    }

    pub fn all(&self) -> &[M] {
        &self.models
    }

    /// Opens a related batch, loading the first relationship if needed.
    pub async fn relatedModels<R>(
        &mut self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<RelatedBatch<'_, <R::Output as RelatedModels>::Model>>
    where
        R: Relation<Parent = M>,
        R::Output: RelatedModels,
    {
        self.loadMissing(name, relation).await?;
        let results = self
            .models
            .iter()
            .map(|model| {
                model
                    .getRelation::<R::Output>(name)
                    .ok_or(DatabaseError::InvalidQuery("relationship output type"))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        Ok(RelatedBatch::new(
            results.into_iter().flat_map(RelatedModels::models),
        ))
    }

    /// Opens a batch containing only the selected polymorphic target type.
    pub async fn morphModels<T: Model>(
        &mut self,
        name: &str,
        relation: &MorphTo<M>,
        alias: &str,
    ) -> DatabaseResult<RelatedBatch<'_, T>> {
        relation.validate_target::<T>(alias)?;
        self.loadMissing(name, relation).await?;
        let results = self
            .models
            .iter()
            .map(|model| {
                model
                    .getRelation::<Option<MorphTarget>>(name)
                    .ok_or(DatabaseError::InvalidQuery("relationship output type"))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        RelatedBatch::from_morph_targets(results.into_iter().filter_map(Option::as_ref), alias)
    }

    /// Loads a relationship on one polymorphic target group.
    pub async fn loadMorph<T: Model>(
        &mut self,
        name: &str,
        morph: &MorphTo<M>,
        alias: &str,
        related_name: &str,
        relation: &impl Relation<Parent = T>,
    ) -> DatabaseResult<RelatedBatch<'_, T>> {
        let mut targets = self.morphModels::<T>(name, morph, alias).await?;
        targets.load_missing(related_name, relation).await?;
        Ok(targets)
    }

    /// Loads a relationship count on one polymorphic target group.
    pub async fn loadMorphCount<T: Model>(
        &mut self,
        name: &str,
        morph: &MorphTo<M>,
        alias: &str,
        related_name: &str,
        relation: &impl ExistenceRelation<Parent = T>,
    ) -> DatabaseResult<RelatedBatch<'_, T>> {
        let mut targets = self.morphModels::<T>(name, morph, alias).await?;
        targets.loadCount(related_name, relation).await?;
        Ok(targets)
    }

    pub fn modelKeys(&self) -> Vec<Value> {
        self.models
            .iter()
            .map(|model| model.getKey().cloned().unwrap_or(Value::Null))
            .collect()
    }

    pub fn find(&self, key: &Value) -> DatabaseResult<Option<&M>> {
        let Some(key) = dictionary_key(Some(key))? else {
            return Ok(None);
        };
        for model in &self.models {
            if dictionary_key(model.getKey())?.as_ref() == Some(&key) {
                return Ok(Some(model));
            }
        }
        Ok(None)
    }

    pub fn contains(&self, key: &Value) -> DatabaseResult<bool> {
        Ok(self.find(key)?.is_some())
    }

    pub fn only(self, keys: &[Value]) -> DatabaseResult<Self> {
        self.filter_keys(keys, true)
    }

    pub fn except(self, keys: &[Value]) -> DatabaseResult<Self> {
        self.filter_keys(keys, false)
    }

    fn filter_keys(self, keys: &[Value], keep: bool) -> DatabaseResult<Self> {
        let mut requested = HashSet::new();
        for key in keys {
            if let Some(key) = dictionary_key(Some(key))? {
                requested.insert(key);
            }
        }
        let mut models = model_dictionary(self.models)?;
        models.retain(|key, _| requested.contains(key) == keep);
        Ok(Self {
            models: models.into_values().collect(),
            autoload: self.autoload,
        })
    }

    pub fn merge(self, models: impl IntoIterator<Item = M>) -> DatabaseResult<Self> {
        let dictionary = model_dictionary(self.models.into_iter().chain(models))?;
        Ok(Self {
            models: dictionary.into_values().collect(),
            autoload: self.autoload,
        })
    }

    pub fn unique(self) -> DatabaseResult<Self> {
        self.merge(std::iter::empty())
    }

    pub fn diff(self, other: &Self) -> DatabaseResult<Self> {
        self.compare_keys(other, false)
    }

    pub fn intersect(self, other: &Self) -> DatabaseResult<Self> {
        self.compare_keys(other, true)
    }

    fn compare_keys(self, other: &Self, keep: bool) -> DatabaseResult<Self> {
        let mut keys = HashSet::new();
        for model in other.iter() {
            if let Some(key) = dictionary_key(model.getKey())? {
                keys.insert(key);
            }
        }
        let mut models = Vec::new();
        for model in self.models {
            let key = dictionary_key(model.getKey())?;
            if key.as_ref().is_some_and(|key| keys.contains(key)) == keep {
                models.push(model);
            }
        }
        Ok(Self {
            models,
            autoload: self.autoload,
        })
    }

    pub fn toQuery(&self) -> DatabaseResult<Builder<'static, M>> {
        let connection = collection_connection(self.iter())?
            .ok_or(DatabaseError::InvalidQuery("empty model collection"))?;
        let mut keys = Vec::with_capacity(self.models.len());
        for model in &self.models {
            let key = model
                .getKey()
                .filter(|key| !matches!(key, Value::Null))
                .ok_or(DatabaseError::InvalidQuery("model collection key"))?;
            keys.push(key.clone());
        }
        let mut query = Builder::<M>::new(connection).withoutGlobalScopes();
        query.whereIn(M::metadata().primary_key, keys);
        Ok(query)
    }

    /// Re-reads existing models without parent scopes, in the original collection order.
    pub async fn fresh(&self) -> DatabaseResult<Self> {
        self.fresh_with_defaults(&[]).await
    }

    async fn fresh_with_defaults(&self, except: &[&str]) -> DatabaseResult<Self> {
        let existing = self.iter().filter(|model| model.state().exists());
        let Some(connection) = collection_connection(existing.clone())? else {
            return Ok(Self {
                models: Vec::new(),
                autoload: self.autoload,
            });
        };
        let mut keys = Vec::new();
        let mut bindings = IndexMap::new();
        for value in existing.filter_map(Model::getKey) {
            if let Some(key) = dictionary_key(Some(value))? {
                bindings.entry(key.clone()).or_insert_with(|| value.clone());
                keys.push(key);
            }
        }
        let bindings: Vec<_> = bindings.into_values().collect();
        let mut refreshed = HashMap::new();
        for batch in bindings.chunks(500) {
            let mut query = M::queryOn(&connection)
                .withOnly(|relations| relations)
                .withoutGlobalScopes();
            query.whereIn(M::metadata().primary_key, batch.to_vec());
            for model in query.get().await? {
                if let Some(key) = dictionary_key(model.getKey())? {
                    refreshed.insert(key, model);
                }
            }
        }
        let mut positions: HashMap<&Vec<u8>, usize> = HashMap::new();
        let mut models = Vec::with_capacity(keys.len());
        for key in keys.iter().rev() {
            if let Some(model) = refreshed.remove(key) {
                positions.insert(key, models.len());
                models.push(model);
            } else if let Some(&position) = positions.get(key) {
                let original = models[position].state();
                let state = ModelState::hydrated_attributes(
                    original.attributes.clone(),
                    connection.clone(),
                );
                models.push(M::from_state(state));
            }
        }
        models.reverse();
        let defaults = M::withDefaultRelations(EagerLoads::default()).without(except);
        has_relationships::load_eager(defaults, &mut models).await?;
        Ok(Self {
            models,
            autoload: self.autoload,
        })
    }

    /// Re-reads the collection and batch-loads the requested relationship.
    pub async fn freshWith<R: Relation<Parent = M>>(
        &self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<Self> {
        let mut refreshed = self.fresh_with_defaults(&[name]).await?;
        refreshed.load(name, relation).await?;
        Ok(refreshed)
    }

    /// Fetches a relationship aggregate and installs it as an original attribute.
    pub async fn loadAggregate(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        aggregate: RelationAggregate<'_>,
    ) -> DatabaseResult<()> {
        self.loadAggregates(&[RelationAggregateQuery::new(name, relation, aggregate)])
            .await
    }

    /// Loads multiple relationship aggregates together for each batch of models.
    pub async fn loadAggregates(
        &mut self,
        aggregates: &[RelationAggregateQuery<'_, M>],
    ) -> DatabaseResult<()> {
        if aggregates.is_empty() {
            return Ok(());
        }
        let models: Vec<_> = self.models.iter().collect();
        let values = aggregate_attributes(&models, aggregates).await?;
        for (model, attributes) in self.models.iter_mut().zip(values) {
            let state = model.state_mut();
            state.apply_attributes(attributes.clone());
            state.original.extend(attributes);
        }
        Ok(())
    }

    pub async fn loadCount(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<()> {
        self.loadAggregate(name, relation, RelationAggregate::Count)
            .await
    }

    pub async fn loadExists(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<()> {
        self.loadAggregate(name, relation, RelationAggregate::Exists)
            .await
    }

    pub async fn loadSum(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<()> {
        self.loadAggregate(name, relation, RelationAggregate::Sum(column))
            .await
    }

    pub async fn loadAvg(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<()> {
        self.loadAggregate(name, relation, RelationAggregate::Avg(column))
            .await
    }

    pub async fn loadMin(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<()> {
        self.loadAggregate(name, relation, RelationAggregate::Min(column))
            .await
    }

    pub async fn loadMax(
        &mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<()> {
        self.loadAggregate(name, relation, RelationAggregate::Max(column))
            .await
    }

    pub async fn load<R: Relation<Parent = M>>(
        &mut self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<()> {
        has_relationships::load(&mut self.models, name, relation, false).await
    }

    pub async fn loadMissing<R: Relation<Parent = M>>(
        &mut self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<()> {
        has_relationships::load(&mut self.models, name, relation, true).await
    }
}

pub(super) async fn aggregate_attributes<M: Model>(
    models: &[&M],
    aggregates: &[RelationAggregateQuery<'_, M>],
) -> DatabaseResult<Vec<Attributes>> {
    if models.is_empty() || aggregates.is_empty() {
        return Ok(Vec::new());
    }
    let primary_key = M::metadata().primary_key;
    let mut aliases = Vec::with_capacity(aggregates.len());
    let mut used = HashSet::from([primary_key.to_ascii_lowercase()]);
    for aggregate in aggregates {
        let alias = aggregate.alias()?;
        if !used.insert(alias.to_ascii_lowercase()) {
            return Err(DatabaseError::InvalidQuery("duplicate aggregate alias"));
        }
        aliases.push(alias);
    }
    let mut keys = Vec::with_capacity(models.len());
    let mut bindings = IndexMap::new();
    for model in models {
        let value = model
            .getKey()
            .filter(|_| model.state().exists())
            .ok_or(DatabaseError::InvalidQuery("model collection key"))?;
        let key = dictionary_key(Some(value))?
            .ok_or(DatabaseError::InvalidQuery("model collection key"))?;
        bindings.entry(key.clone()).or_insert_with(|| value.clone());
        keys.push(key);
    }
    let connection = collection_connection(models.iter().copied())?
        .ok_or(DatabaseError::InvalidQuery("empty model collection"))?;
    let bindings: Vec<_> = bindings.into_values().collect();
    let mut values = HashMap::new();
    for batch in bindings.chunks(500) {
        let mut query = Builder::<M>::new(connection.clone())
            .withoutGlobalScopes()
            .select([primary_key]);
        for aggregate in aggregates {
            query = aggregate.apply(query)?;
        }
        query.whereIn(primary_key, batch.to_vec());
        for model in query.get().await? {
            if let Some(key) = dictionary_key(model.getKey())? {
                let attributes = aliases
                    .iter()
                    .map(|alias| {
                        let value = model
                            .getAttribute(alias)
                            .ok_or(DatabaseError::InvalidQuery("missing aggregate projection"))?;
                        Ok((alias.clone(), value.clone()))
                    })
                    .collect::<DatabaseResult<Attributes>>()?;
                values.insert(key, attributes);
            }
        }
    }
    keys.iter()
        .map(|key| {
            values
                .get(key)
                .cloned()
                .ok_or(DatabaseError::ModelNotFound(M::metadata().table))
        })
        .collect()
}

fn collection_connection<'a, M: Model>(
    mut models: impl Iterator<Item = &'a M>,
) -> DatabaseResult<Option<Connection>> {
    let Some(first) = models.next() else {
        return Ok(None);
    };
    let connection = match &first.state().connection {
        Some(connection) => connection.clone(),
        None => DB::connection(M::metadata().connection)?,
    };
    for model in models {
        if let Some(source) = &model.state().connection
            && !connection.same_query_source(source)
        {
            return Err(DatabaseError::InvalidQuery(
                "mixed model collection connections",
            ));
        }
    }
    Ok(Some(connection))
}

fn model_dictionary<M: Model>(
    models: impl IntoIterator<Item = M>,
) -> DatabaseResult<IndexMap<Vec<u8>, M>> {
    let mut dictionary = IndexMap::new();
    for model in models {
        if let Some(key) = dictionary_key(model.getKey())? {
            dictionary.insert(key, model);
        }
    }
    Ok(dictionary)
}

impl<M: Model> Deref for Collection<M> {
    type Target = [M];

    fn deref(&self) -> &Self::Target {
        self.all()
    }
}

impl<M: Model> IntoIterator for Collection<M> {
    type Item = M;
    type IntoIter = std::vec::IntoIter<M>;

    fn into_iter(self) -> Self::IntoIter {
        self.models.into_iter()
    }
}
