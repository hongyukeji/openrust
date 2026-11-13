use std::{
    collections::{HashMap, HashSet},
    marker::PhantomData,
    ops::Deref,
    sync::Arc,
};

use indexmap::IndexMap;

use crate::database::{Connection, DB, DatabaseError, DatabaseFuture, DatabaseResult, Value};
use crate::support::{ArrayKey, Collection as BaseCollection, CollectionResult};

use super::{
    Attributes, Builder, EagerLoads, Model, ModelState, NamedRelations, RelatedBatch,
    RelatedModels, RelationAggregate, RelationAggregateQuery,
    concerns::has_relationships,
    relations::{ExistenceRelation, MorphTarget, MorphTo, Relation, relation::dictionary_key},
};

/// A collection of hydrated models, in query order.
pub struct Collection<M: Model> {
    models: Vec<M>,
    keys: Option<Vec<ArrayKey>>,
    autoload: bool,
}

/// A model borrowed from its collection for relationship access.
pub struct CollectionModel<'a, M: Model> {
    collection: &'a mut Collection<M>,
    index: usize,
}

trait Update<M>: Send {
    fn run<'a>(
        &'a mut self,
        models: &'a mut [M],
        selected: usize,
        autoload: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

trait ModelSource<M: Model>: Send + Sync {
    fn selected(&self) -> &M;
    fn update<'a>(
        &'a mut self,
        update: &'a mut dyn Update<M>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

impl<M: Model> ModelSource<M> for CollectionModel<'_, M> {
    fn selected(&self) -> &M {
        &self.collection[self.index]
    }

    fn update<'a>(
        &'a mut self,
        update: &'a mut dyn Update<M>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        update.run(
            &mut self.collection.models,
            self.index,
            self.collection.autoload,
        )
    }
}

impl<M: Model> Deref for CollectionModel<'_, M> {
    type Target = M;

    fn deref(&self) -> &M {
        self.selected()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<M: Model> CollectionModel<'_, M> {
    /// Reads a relationship using the owning collection's autoload policy.
    pub async fn getRelationOrLoad<R: Relation<Parent = M>>(
        &mut self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<&R::Output> {
        load_from_source(self, name, relation).await
    }

    /// Opens a single related model while retaining access to the root collection.
    pub async fn relatedModelAt<'a, R>(
        &'a mut self,
        name: &str,
        relation: &'a R,
        index: usize,
    ) -> DatabaseResult<Option<RelatedCollectionModel<'a, M, R>>>
    where
        R: Relation<Parent = M>,
        R::Output: RelatedModels,
    {
        open_child(self, name, relation, index, Ordinary).await
    }

    /// Borrows a polymorphic target while retaining access to the root collection.
    pub async fn morphModel<'a, T: Model>(
        &'a mut self,
        name: &str,
        relation: &'a MorphTo<M>,
        alias: &'a str,
    ) -> DatabaseResult<Option<MorphCollectionModel<'a, M, T>>> {
        relation.validate_target::<T>(alias)?;
        open_child(
            self,
            name,
            relation,
            0,
            Morph {
                alias,
                model: PhantomData,
            },
        )
        .await
    }
}

/// A related model borrowed through its root collection.
pub struct BorrowedRelatedModel<'a, T: Model, P = ()> {
    source: Box<dyn ModelSource<T> + 'a>,
    parent: PhantomData<fn() -> P>,
}

pub type RelatedCollectionModel<'a, M, R> =
    BorrowedRelatedModel<'a, <<R as Relation>::Output as RelatedModels>::Model, (M, R)>;

pub type MorphCollectionModel<'a, M, T> = BorrowedRelatedModel<'a, T, M>;

impl<T: Model, P> Deref for BorrowedRelatedModel<'_, T, P> {
    type Target = T;

    fn deref(&self) -> &T {
        self.source.selected()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<T: Model, P> BorrowedRelatedModel<'_, T, P> {
    /// Accesses a descendant and writes its loaded relationship back to the root collection.
    pub async fn getRelationOrLoad<R: Relation<Parent = T>>(
        &mut self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<&R::Output> {
        load_from_source(self.source.as_mut(), name, relation).await
    }

    /// Continues through another ordinary relationship.
    pub async fn relatedModelAt<'a, R>(
        &'a mut self,
        name: &str,
        relation: &'a R,
        index: usize,
    ) -> DatabaseResult<Option<RelatedCollectionModel<'a, T, R>>>
    where
        R: Relation<Parent = T>,
        R::Output: RelatedModels,
    {
        open_child(self.source.as_mut(), name, relation, index, Ordinary).await
    }

    /// Continues through a polymorphic relationship of the selected alias.
    pub async fn morphModel<'a, U: Model>(
        &'a mut self,
        name: &str,
        relation: &'a MorphTo<T>,
        alias: &'a str,
    ) -> DatabaseResult<Option<MorphCollectionModel<'a, T, U>>> {
        relation.validate_target::<U>(alias)?;
        open_child(
            self.source.as_mut(),
            name,
            relation,
            0,
            Morph {
                alias,
                model: PhantomData,
            },
        )
        .await
    }
}

async fn load_from_source<'a, M: Model, R: Relation<Parent = M>>(
    source: &'a mut dyn ModelSource<M>,
    name: &str,
    relation: &R,
) -> DatabaseResult<&'a R::Output> {
    if !source.selected().relationLoaded(name) {
        source.update(&mut LoadRelation { name, relation }).await?;
    }
    source
        .selected()
        .getRelation(name)
        .ok_or(DatabaseError::InvalidQuery("relationship output type"))
}

struct LoadRelation<'a, R> {
    name: &'a str,
    relation: &'a R,
}

impl<R: Relation> Update<R::Parent> for LoadRelation<'_, R> {
    fn run<'a>(
        &'a mut self,
        models: &'a mut [R::Parent],
        selected: usize,
        autoload: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if autoload {
                has_relationships::load(models, self.name, self.relation, true).await
            } else {
                has_relationships::load_on_access(&mut models[selected], self.name, self.relation)
                    .await
            }
        })
    }
}

trait RelationOutput<O>: Send + Sync {
    type Child: Model;

    fn children<'a>(&self, output: &'a O) -> DatabaseResult<Vec<&'a Self::Child>>;
    fn replace(
        &self,
        output: &O,
        replacements: &HashMap<usize, Arc<Self::Child>>,
    ) -> DatabaseResult<O>;
}

struct Ordinary;

impl<O: RelatedModels> RelationOutput<O> for Ordinary {
    type Child = O::Model;

    fn children<'a>(&self, output: &'a O) -> DatabaseResult<Vec<&'a Self::Child>> {
        Ok(output.models().iter().map(Arc::as_ref).collect())
    }

    fn replace(
        &self,
        output: &O,
        replacements: &HashMap<usize, Arc<Self::Child>>,
    ) -> DatabaseResult<O> {
        output.replace_models(|model| {
            replacements
                .get(&Arc::as_ptr(model).addr())
                .cloned()
                .unwrap_or_else(|| model.clone())
        })
    }
}

struct Morph<'a, T> {
    alias: &'a str,
    model: PhantomData<T>,
}

impl<T: Model> RelationOutput<Option<MorphTarget>> for Morph<'_, T> {
    type Child = T;

    fn children<'a>(&self, output: &'a Option<MorphTarget>) -> DatabaseResult<Vec<&'a T>> {
        match output
            .as_ref()
            .filter(|target| target.alias() == self.alias)
        {
            Some(target) => target.downcast_ref::<T>().map(|model| vec![model]).ok_or(
                DatabaseError::InvalidQuery("morph type does not match model"),
            ),
            None => Ok(Vec::new()),
        }
    }

    fn replace(
        &self,
        output: &Option<MorphTarget>,
        replacements: &HashMap<usize, Arc<T>>,
    ) -> DatabaseResult<Option<MorphTarget>> {
        let mut output = output.clone();
        if let Some(target) = output
            .as_mut()
            .filter(|target| target.alias() == self.alias)
            && let Some(model) = target.downcast_ref::<T>()
            && let Some(replacement) = replacements.get(&std::ptr::from_ref(model).addr())
        {
            target.replace_model(replacement.clone());
        }
        Ok(output)
    }
}

async fn open_child<'a, M, R, E, P>(
    parent: &'a mut dyn ModelSource<M>,
    name: &str,
    relation: &'a R,
    index: usize,
    edge: E,
) -> DatabaseResult<Option<BorrowedRelatedModel<'a, E::Child, P>>>
where
    M: Model,
    R: Relation<Parent = M>,
    E: RelationOutput<R::Output> + 'a,
{
    let output = load_from_source(parent, name, relation).await?;
    if index >= edge.children(output)?.len() {
        return Ok(None);
    }
    Ok(Some(BorrowedRelatedModel {
        source: Box::new(RelationSource {
            parent,
            name: name.to_owned(),
            relation,
            index,
            edge,
        }),
        parent: PhantomData,
    }))
}

struct RelationSource<'a, M: Model, R, E> {
    parent: &'a mut dyn ModelSource<M>,
    name: String,
    relation: &'a R,
    index: usize,
    edge: E,
}

impl<M, R, E> ModelSource<E::Child> for RelationSource<'_, M, R, E>
where
    M: Model,
    R: Relation<Parent = M>,
    E: RelationOutput<R::Output>,
{
    fn selected(&self) -> &E::Child {
        let output = self
            .parent
            .selected()
            .getRelation::<R::Output>(&self.name)
            .expect("related output remains installed");
        self.edge
            .children(output)
            .expect("related output remains valid")[self.index]
    }

    fn update<'a>(
        &'a mut self,
        update: &'a mut dyn Update<E::Child>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            self.parent
                .update(&mut UpdateChildren {
                    name: &self.name,
                    relation: self.relation,
                    index: self.index,
                    edge: &self.edge,
                    update,
                })
                .await
        })
    }
}

struct UpdateChildren<'a, R, E: RelationOutput<R::Output>>
where
    R: Relation,
{
    name: &'a str,
    relation: &'a R,
    index: usize,
    edge: &'a E,
    update: &'a mut dyn Update<E::Child>,
}

impl<R, E> Update<R::Parent> for UpdateChildren<'_, R, E>
where
    R: Relation,
    E: RelationOutput<R::Output>,
{
    fn run<'a>(
        &'a mut self,
        models: &'a mut [R::Parent],
        selected: usize,
        autoload: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let Self {
                name,
                relation,
                index,
                edge,
                update,
            } = self;
            let (models, selected) = if autoload {
                has_relationships::load(models, name, *relation, true).await?;
                (models, selected)
            } else {
                (std::slice::from_mut(&mut models[selected]), 0)
            };
            let outputs = models
                .iter()
                .map(|model| {
                    model
                        .getRelation::<R::Output>(name)
                        .ok_or(DatabaseError::InvalidQuery("relationship output type"))
                })
                .collect::<DatabaseResult<Vec<_>>>()?;
            let selected_model = edge.children(outputs[selected])?[*index];
            let selected_address = std::ptr::from_ref(selected_model).addr();
            let mut copies = IndexMap::new();
            if autoload {
                for output in &outputs {
                    for child in edge.children(output)? {
                        copies
                            .entry(std::ptr::from_ref(child).addr())
                            .or_insert_with(|| E::Child::from_state(child.state().clone()));
                    }
                }
            } else {
                copies.insert(
                    selected_address,
                    E::Child::from_state(selected_model.state().clone()),
                );
            }
            let selected = copies
                .get_index_of(&selected_address)
                .expect("selected model is in the batch");
            let (pointers, mut copies): (Vec<_>, Vec<_>) = copies.into_iter().unzip();
            update.run(&mut copies, selected, autoload).await?;
            let replacements = pointers
                .into_iter()
                .zip(copies.into_iter().map(Arc::new))
                .collect();
            let outputs = outputs
                .into_iter()
                .map(|output| edge.replace(output, &replacements))
                .collect::<DatabaseResult<Vec<_>>>()?;
            for (model, output) in models.iter_mut().zip(outputs) {
                model
                    .state_mut()
                    .relations
                    .insert_relation::<R>((*name).to_owned(), output);
            }
            Ok(())
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent Collection API")]
impl<M: Model> Collection<M> {
    pub fn new(models: Vec<M>) -> Self {
        Self {
            models,
            keys: None,
            autoload: M::automaticallyEagerLoadRelationships(),
        }
    }

    /// Builds an Eloquent collection with explicit collection keys.
    pub fn fromEntries<K: Into<ArrayKey>>(entries: impl IntoIterator<Item = (K, M)>) -> Self {
        let (keys, models) = BaseCollection::fromEntries(entries).into_iter().unzip();
        Self {
            models,
            keys: Some(keys),
            autoload: M::automaticallyEagerLoadRelationships(),
        }
    }

    /// Looks up a collection key independently of a model's primary key or position.
    pub fn getByKey(&self, key: impl Into<ArrayKey>) -> Option<&M> {
        let key = key.into().normalized();
        if let Some(keys) = &self.keys {
            return keys
                .iter()
                .position(|candidate| *candidate == key)
                .map(|index| &self.models[index]);
        }
        match key {
            ArrayKey::Integer(index) => usize::try_from(index)
                .ok()
                .and_then(|index| self.models.get(index)),
            ArrayKey::String(_) => None,
        }
    }

    fn key_at(&self, index: usize) -> ArrayKey {
        self.keys.as_ref().map_or_else(
            || ArrayKey::Integer(i64::try_from(index).expect("collection index fits i64")),
            |keys| keys[index].clone(),
        )
    }

    /// Batch-loads missing relationships when accessed through this collection.
    #[must_use]
    pub fn withRelationshipAutoloading(mut self) -> Self {
        self.autoload = true;
        self
    }

    /// Borrows a model with access to its collection for batch relationship loading.
    pub fn modelAt(&mut self, index: usize) -> Option<CollectionModel<'_, M>> {
        (index < self.models.len()).then_some(CollectionModel {
            collection: self,
            index,
        })
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

    /// Converts the models into a base collection, in their current order.
    pub fn toBase(mut self) -> BaseCollection<M> {
        if self.autoload {
            super::detached_autoload::attach(&mut self.models);
        }
        self.into_base()
    }

    fn into_base(self) -> BaseCollection<M> {
        match self.keys {
            Some(keys) => BaseCollection::fromEntries(keys.into_iter().zip(self.models)),
            None => self.models.into_iter().collect(),
        }
    }

    /// Maps each model and its index into a base collection.
    pub fn map<T>(&self, mut callback: impl FnMut(&M, usize) -> T) -> BaseCollection<T> {
        BaseCollection::fromEntries(
            self.iter()
                .enumerate()
                .map(|(index, model)| (self.key_at(index), callback(model, index))),
        )
    }

    /// Maps owned models into another Eloquent collection.
    pub fn mapModels<N: Model>(self, mut callback: impl FnMut(M, usize) -> N) -> Collection<N> {
        let models = self
            .models
            .into_iter()
            .enumerate()
            .map(|(index, model)| callback(model, index))
            .collect();
        Collection {
            models,
            keys: self.keys,
            autoload: self.autoload || N::automaticallyEagerLoadRelationships(),
        }
    }

    /// Maps models into a keyed Eloquent collection.
    pub fn mapModelsWithKeys<K, N, I>(
        self,
        mut callback: impl FnMut(M, ArrayKey) -> I,
    ) -> Collection<N>
    where
        K: Into<ArrayKey>,
        N: Model,
        I: IntoIterator<Item = (K, N)>,
    {
        let autoload = self.autoload;
        let mut result = Collection::fromEntries(
            self.into_base()
                .into_iter()
                .flat_map(|(key, model)| callback(model, key)),
        );
        result.autoload |= autoload;
        result
    }

    pub fn mapWithKeys<K, T, I>(
        &self,
        mut callback: impl FnMut(&M, usize) -> I,
    ) -> BaseCollection<T>
    where
        K: Into<ArrayKey>,
        I: IntoIterator<Item = (K, T)>,
    {
        BaseCollection::fromEntries(
            self.iter()
                .enumerate()
                .flat_map(|(index, model)| callback(model, index)),
        )
    }

    pub fn pluck<T>(&self, project: impl FnMut(&M) -> T) -> BaseCollection<T> {
        self.iter().map(project).collect()
    }

    pub fn pluckWithKeys<K: Into<ArrayKey>, T>(
        &self,
        mut key: impl FnMut(&M) -> K,
        mut project: impl FnMut(&M) -> T,
    ) -> BaseCollection<T> {
        BaseCollection::fromEntries(self.iter().map(|model| (key(model), project(model))))
    }

    /// Returns the collection indices as a base collection.
    pub fn keys(&self) -> BaseCollection<ArrayKey> {
        (0..self.len()).map(|index| self.key_at(index)).collect()
    }

    /// Combines models and values by position, including any unpaired values.
    pub fn zip<T>(
        self,
        items: impl IntoIterator<Item = T>,
    ) -> BaseCollection<(Option<M>, Option<T>)> {
        self.toBase().zip(items)
    }

    /// Combines models with any number of additional columns by position.
    pub fn zipMany<T, I>(
        self,
        columns: impl IntoIterator<Item = I>,
    ) -> BaseCollection<(Option<M>, Vec<Option<T>>)>
    where
        I: IntoIterator<Item = T>,
    {
        self.toBase().zipMany(columns)
    }

    /// Exchanges projected scalar keys with the original collection keys.
    pub fn flipBy<K: Into<ArrayKey>>(
        &self,
        mut project: impl FnMut(&M) -> Option<K>,
    ) -> BaseCollection<ArrayKey> {
        BaseCollection::fromEntries(self.iter().enumerate().filter_map(|(index, model)| {
            project(model).map(|value| (self.key_at(index), value.into()))
        }))
        .flip()
    }

    /// Pads the base collection with independent snapshots of the given model.
    pub fn pad(self, size: isize, model: &M) -> CollectionResult<BaseCollection<M>> {
        self.toBase()
            .pad_with(size, || M::from_state(model.state().clone()))
    }

    /// Returns a base collection with consecutive integer keys.
    pub fn flatten(self) -> BaseCollection<M> {
        self.into_iter().collect()
    }

    pub fn countBy<K: Into<ArrayKey>>(
        &self,
        mut group: impl FnMut(&M, &ArrayKey) -> K,
    ) -> BaseCollection<usize> {
        let models = BaseCollection::fromEntries(
            self.iter()
                .enumerate()
                .map(|(index, model)| (self.key_at(index), model)),
        );
        models.countBy(|model, key| group(model, key))
    }

    /// Splits models into matching and nonmatching groups, preserving collection keys.
    pub fn partition(self, predicate: impl FnMut(&M, &ArrayKey) -> bool) -> (Self, Self) {
        let autoload = self.autoload;
        let (matches, rest) = self.into_base().partition(predicate);
        (
            Self {
                autoload,
                ..Self::fromEntries(matches)
            },
            Self {
                autoload,
                ..Self::fromEntries(rest)
            },
        )
    }

    pub fn makeHidden(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.makeHidden(attributes);
        }
        self
    }

    pub fn setHidden(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.setHidden(attributes);
        }
        self
    }

    pub fn mergeHidden(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.mergeHidden(attributes);
        }
        self
    }

    pub fn makeVisible(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.makeVisible(attributes);
        }
        self
    }

    pub fn setVisible(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.setVisible(attributes);
        }
        self
    }

    pub fn mergeVisible(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.mergeVisible(attributes);
        }
        self
    }

    pub fn append(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.append(attributes);
        }
        self
    }

    pub fn setAppends(&mut self, attributes: &[&str]) -> &mut Self {
        for model in &mut self.models {
            model.setAppends(attributes);
        }
        self
    }

    pub fn withoutAppends(&mut self) -> &mut Self {
        self.setAppends(&[])
    }

    pub fn toArray(&self) -> DatabaseResult<Vec<serde_json::Map<String, serde_json::Value>>> {
        self.iter().map(Model::toArray).collect()
    }

    pub fn toJson(&self) -> DatabaseResult<String> {
        let rows = self.toArray()?;
        match &self.keys {
            Some(keys) => BaseCollection::fromEntries(keys.iter().zip(rows))
                .toJson()
                .map_err(|_| DatabaseError::InvalidQuery("model collection JSON")),
            None => serde_json::to_string(&rows)
                .map_err(|_| DatabaseError::InvalidQuery("model collection JSON")),
        }
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
        load_morph_relation(&mut self.models, name, morph, alias, related_name, relation).await?;
        self.morphModels::<T>(name, morph, alias).await
    }

    /// Reloads the registered relationship paths for one polymorphic target group.
    pub async fn loadMorphPaths<T: Model>(
        &mut self,
        name: &str,
        morph: &MorphTo<M>,
        alias: &str,
        paths: &[&str],
    ) -> DatabaseResult<()> {
        load_morph_paths::<M, T>(&mut self.models, name, morph, alias, paths).await
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
        self.loadMorphAggregate(
            name,
            morph,
            alias,
            RelationAggregateQuery::new(related_name, relation, RelationAggregate::Count),
        )
        .await?;
        self.morphModels::<T>(name, morph, alias).await
    }

    /// Loads an aggregate onto the cached models of one polymorphic target group.
    pub async fn loadMorphAggregate<T: Model>(
        &mut self,
        name: &str,
        morph: &MorphTo<M>,
        alias: &str,
        aggregate: RelationAggregateQuery<'_, T>,
    ) -> DatabaseResult<()> {
        load_morph_aggregates(&mut self.models, name, morph, alias, &[aggregate]).await
    }

    /// Loads several aggregates in a shared query for each batch of target keys.
    pub async fn loadMorphAggregates<T: Model>(
        &mut self,
        name: &str,
        morph: &MorphTo<M>,
        alias: &str,
        aggregates: &[RelationAggregateQuery<'_, T>],
    ) -> DatabaseResult<()> {
        load_morph_aggregates(&mut self.models, name, morph, alias, aggregates).await
    }

    pub fn modelKeys(&self) -> Vec<Value> {
        self.models
            .iter()
            .map(|model| model.getKey().cloned().unwrap_or(Value::Null))
            .collect()
    }

    /// Reports the compiled model type for a nonempty collection.
    pub fn getQueueableClass(&self) -> Option<&'static str> {
        self.first().map(|_| std::any::type_name::<M>())
    }

    /// Returns model identifiers in collection order, including duplicates.
    pub fn getQueueableIds(&self) -> Vec<Value> {
        self.iter()
            .map(|model| model.getQueueableId().cloned().unwrap_or(Value::Null))
            .collect()
    }

    /// Returns the loaded relation paths shared by all models.
    pub fn getQueueableRelations(&self) -> Vec<String> {
        super::queueable::common_relations(self.iter())
    }

    /// Rejects collections spanning multiple database connection names.
    pub fn getQueueableConnection(&self) -> DatabaseResult<Option<&str>> {
        let Some(first) = self.first() else {
            return Ok(None);
        };
        let connection = first.getConnectionName();
        if self
            .iter()
            .skip(1)
            .any(|model| model.getConnectionName() != connection)
        {
            return Err(DatabaseError::InvalidQuery(
                "queueable collection has multiple connections",
            ));
        }
        Ok(connection)
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

    pub fn doesntContain(&self, key: &Value) -> DatabaseResult<bool> {
        Ok(!self.contains(key)?)
    }

    /// Checks model identity using the key, table, and connection name.
    pub fn containsModel(&self, model: &impl Model) -> bool {
        self.iter().any(|candidate| candidate.is(model))
    }

    pub fn doesntContainModel(&self, model: &impl Model) -> bool {
        !self.containsModel(model)
    }

    /// Checks whether a model and its collection index satisfy the predicate.
    pub fn containsWhere(&self, mut predicate: impl FnMut(&M, usize) -> bool) -> bool {
        self.iter()
            .enumerate()
            .any(|(index, model)| predicate(model, index))
    }

    pub fn doesntContainWhere(&self, predicate: impl FnMut(&M, usize) -> bool) -> bool {
        !self.containsWhere(predicate)
    }

    /// Finds a model by the supplied model's primary key.
    pub fn findModel(&self, model: &impl Model) -> DatabaseResult<Option<&M>> {
        match model.getKey() {
            Some(key) => self.find(key),
            None => Ok(None),
        }
    }

    pub fn findModelOrFail(&self, model: &impl Model) -> DatabaseResult<&M> {
        self.findModel(model)?
            .ok_or(DatabaseError::ModelNotFound(M::metadata().table))
    }

    pub fn findOrFail(&self, key: &Value) -> DatabaseResult<&M> {
        self.find(key)?
            .ok_or(DatabaseError::ModelNotFound(M::metadata().table))
    }

    /// Selects matching models from this collection in collection order.
    pub fn findMany(&self, keys: &[Value]) -> DatabaseResult<Vec<&M>> {
        self.find_many(&Self::lookup_keys(keys)?)
    }

    /// Selects matching models, returning an error if a requested key is missing.
    pub fn findManyOrFail(&self, keys: &[Value]) -> DatabaseResult<Vec<&M>> {
        let mut remaining = Self::lookup_keys(keys)?;
        let models = self.find_many(&remaining)?;
        for model in &models {
            if let Some(key) = dictionary_key(model.getKey())? {
                remaining.remove(&key);
            }
        }
        if keys.contains(&Value::Null) || !remaining.is_empty() {
            return Err(DatabaseError::ModelNotFound(M::metadata().table));
        }
        Ok(models)
    }

    fn find_many(&self, keys: &HashSet<Vec<u8>>) -> DatabaseResult<Vec<&M>> {
        let mut models = Vec::new();
        if !keys.is_empty() {
            for model in &self.models {
                if dictionary_key(model.getKey())?.is_some_and(|key| keys.contains(&key)) {
                    models.push(model);
                }
            }
        }
        Ok(models)
    }

    pub fn only(self, keys: &[Value]) -> DatabaseResult<Self> {
        self.filter_keys(keys, true)
    }

    pub fn except(self, keys: &[Value]) -> DatabaseResult<Self> {
        self.filter_keys(keys, false)
    }

    fn filter_keys(self, keys: &[Value], keep: bool) -> DatabaseResult<Self> {
        let requested = Self::lookup_keys(keys)?;
        let mut models = model_dictionary(self.models, Model::getKey)?;
        models.retain(|key, _| requested.contains(key) == keep);
        Ok(Self {
            models: models.into_values().collect(),
            keys: None,
            autoload: self.autoload,
        })
    }

    fn lookup_keys(keys: &[Value]) -> DatabaseResult<HashSet<Vec<u8>>> {
        let mut requested = HashSet::with_capacity(keys.len());
        for key in keys {
            if let Some(key) = dictionary_key(Some(key))? {
                requested.insert(key);
            }
        }
        Ok(requested)
    }

    /// Indexes borrowed models by their normalized primary keys.
    ///
    /// Missing keys are skipped. Later models replace earlier values at the same
    /// key without changing its position. Keys retain binary data.
    pub fn getDictionary(&self) -> DatabaseResult<IndexMap<Vec<u8>, &M>> {
        model_dictionary(self.iter(), |model| model.getKey())
    }

    pub fn merge(self, models: impl IntoIterator<Item = M>) -> DatabaseResult<Self> {
        let dictionary = model_dictionary(self.models.into_iter().chain(models), Model::getKey)?;
        Ok(Self {
            models: dictionary.into_values().collect(),
            keys: None,
            autoload: self.autoload,
        })
    }

    pub fn unique(self) -> DatabaseResult<Self> {
        self.merge(std::iter::empty())
    }

    /// Retains the first model for each projected value, preserving collection keys.
    pub fn uniqueBy<K: Eq + std::hash::Hash>(
        self,
        project: impl FnMut(&M, &ArrayKey) -> K,
    ) -> Self {
        let autoload = self.autoload;
        Self {
            autoload,
            ..Self::fromEntries(self.into_base().uniqueBy(project))
        }
    }

    /// Returns subsequent occurrences of each model identity with their original keys.
    pub fn duplicates(self) -> Self {
        let mut duplicates = HashSet::new();
        for (index, model) in self.models.iter().enumerate() {
            if self.models[..index]
                .iter()
                .any(|previous| previous.is(model))
            {
                duplicates.insert(self.key_at(index));
            }
        }
        let autoload = self.autoload;
        let entries = self
            .into_base()
            .into_iter()
            .filter(|(key, _)| duplicates.contains(key));
        Self {
            autoload,
            ..Self::fromEntries(entries)
        }
    }

    pub fn duplicatesStrict(self) -> Self {
        self.duplicates()
    }

    /// Returns repeated projected values with their original collection keys.
    pub fn duplicatesBy<K: PartialEq>(
        &self,
        mut project: impl FnMut(&M) -> K,
    ) -> BaseCollection<K> {
        let mut seen = Vec::new();
        let mut duplicates = BaseCollection::new();
        for (index, model) in self.iter().enumerate() {
            let value = project(model);
            if seen.contains(&value) {
                duplicates.put(self.key_at(index), value);
            } else {
                seen.push(value);
            }
        }
        duplicates
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
            keys: None,
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
        self.fresh_with_loads(M::withDefaultRelations).await
    }

    /// Re-reads the collection and eager loads the registered relationship paths.
    pub async fn freshWithPaths(&self, paths: &[&str]) -> DatabaseResult<Self> {
        if self.is_empty() {
            return self.fresh().await;
        }
        let requested = NamedRelations::<M>::prepare(paths)?;
        self.fresh_with_loads(|defaults| {
            let mut loads = M::withDefaultRelations(defaults);
            loads.extend(requested);
            loads
        })
        .await
    }

    async fn fresh_with_loads(
        &self,
        configure: impl FnOnce(EagerLoads<M>) -> EagerLoads<M>,
    ) -> DatabaseResult<Self> {
        let existing = self
            .iter()
            .enumerate()
            .filter(|(_, model)| model.state().exists());
        let Some(connection) = collection_connection(existing.clone().map(|(_, model)| model))?
        else {
            return Ok(Self {
                models: Vec::new(),
                keys: None,
                autoload: self.autoload,
            });
        };
        let mut keys = Vec::new();
        let mut bindings = IndexMap::new();
        for (index, model) in existing {
            if let Some(value) = model.getKey()
                && let Some(key) = dictionary_key(Some(value))?
            {
                bindings.entry(key.clone()).or_insert_with(|| value.clone());
                keys.push((index, key));
            }
        }
        let bindings: Vec<_> = bindings.into_values().collect();
        let casts = self.models.first().map(|model| &model.state().casts);
        let mut refreshed = HashMap::new();
        for batch in bindings.chunks(500) {
            let mut query = M::queryOn(&connection)
                .withOnly(|relations| relations)
                .withoutGlobalScopes();
            if let Some(casts) = casts {
                query.casts.clone_from(casts);
            }
            query.whereIn(M::metadata().primary_key, batch.to_vec());
            for model in query.get().await? {
                if let Some(key) = dictionary_key(model.getKey())? {
                    refreshed.insert(key, model);
                }
            }
        }
        let mut positions: HashMap<&Vec<u8>, usize> = HashMap::new();
        let mut models = Vec::with_capacity(keys.len());
        let mut retained = self.keys.as_ref().map(|_| Vec::with_capacity(keys.len()));
        for (index, key) in keys.iter().rev() {
            if let Some(model) = refreshed.remove(key) {
                positions.insert(key, models.len());
                models.push(model);
            } else if let Some(&position) = positions.get(key) {
                let original = models[position].state();
                let state = ModelState::hydrated_attributes(
                    original.attributes.clone(),
                    connection.clone(),
                    original.casts.clone(),
                );
                models.push(M::from_state(state));
            } else {
                continue;
            }
            if let Some(retained) = &mut retained {
                retained.push(self.key_at(*index));
            }
        }
        models.reverse();
        if let Some(retained) = &mut retained {
            retained.reverse();
        }
        has_relationships::load_eager(configure(EagerLoads::default()), &mut models).await?;
        Ok(Self {
            models,
            keys: retained,
            autoload: self.autoload,
        })
    }

    /// Re-reads the collection and batch-loads the requested relationship.
    pub async fn freshWith<R: Relation<Parent = M>>(
        &self,
        name: &str,
        relation: &R,
    ) -> DatabaseResult<Self> {
        let mut refreshed = self
            .fresh_with_loads(|defaults| M::withDefaultRelations(defaults).without(&[name]))
            .await?;
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

    /// Eager loads the named paths across this collection.
    pub async fn loadPaths(&mut self, paths: &[&str]) -> DatabaseResult<()> {
        let loads = super::NamedRelations::<M>::prepare(paths)?;
        has_relationships::load_eager(loads, &mut self.models).await
    }

    /// Loads missing relationships along the supplied registered paths.
    pub async fn loadMissingPaths(&mut self, paths: &[&str]) -> DatabaseResult<()> {
        super::NamedRelations::<M>::load_missing(&mut self.models, paths).await
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

pub(super) async fn load_morph_relation<M: Model, T: Model>(
    models: &mut [M],
    name: &str,
    morph: &MorphTo<M>,
    alias: &str,
    related_name: &str,
    relation: &impl Relation<Parent = T>,
) -> DatabaseResult<()> {
    let mut targets = MorphSnapshots::<T>::prepare(models, name, morph, alias).await?;
    has_relationships::load(&mut targets.models, related_name, relation, false).await?;
    targets.install(models, name);
    Ok(())
}

pub(super) async fn load_morph_paths<M: Model, T: Model>(
    models: &mut [M],
    name: &str,
    morph: &MorphTo<M>,
    alias: &str,
    paths: &[&str],
) -> DatabaseResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let loads = NamedRelations::<T>::prepare(paths)?;
    let mut targets = MorphSnapshots::<T>::prepare(models, name, morph, alias).await?;
    has_relationships::load_eager(loads, &mut targets.models).await?;
    targets.install(models, name);
    Ok(())
}

pub(super) async fn load_morph_aggregates<M: Model, T: Model>(
    models: &mut [M],
    name: &str,
    morph: &MorphTo<M>,
    alias: &str,
    aggregates: &[RelationAggregateQuery<'_, T>],
) -> DatabaseResult<()> {
    if models.is_empty() || aggregates.is_empty() {
        return Ok(());
    }
    let mut targets = MorphSnapshots::<T>::prepare(models, name, morph, alias).await?;
    let refs: Vec<_> = targets.models.iter().collect();
    let attributes = aggregate_attributes(&refs, aggregates).await?;
    for (model, attributes) in targets.models.iter_mut().zip(attributes) {
        let state = model.state_mut();
        state.apply_attributes(attributes.clone());
        state.original.extend(attributes);
    }
    targets.install(models, name);
    Ok(())
}

// One working copy per shared target. Install only after all queries succeed.
struct MorphSnapshots<T: Model> {
    models: Vec<T>,
    parents: Vec<(usize, usize, MorphTarget)>,
}

impl<T: Model> MorphSnapshots<T> {
    async fn prepare<M: Model>(
        models: &mut [M],
        name: &str,
        morph: &MorphTo<M>,
        alias: &str,
    ) -> DatabaseResult<Self> {
        morph.validate_target::<T>(alias)?;
        has_relationships::load(models, name, morph, true).await?;
        let mut copies = IndexMap::new();
        let mut parents = Vec::new();
        for (index, parent) in models.iter().enumerate() {
            let output = parent
                .getRelation::<Option<MorphTarget>>(name)
                .ok_or(DatabaseError::InvalidQuery("relationship output type"))?;
            if let Some(target) = output.as_ref().filter(|target| target.alias() == alias) {
                let model = target
                    .downcast_ref::<T>()
                    .ok_or(DatabaseError::InvalidQuery(
                        "morph type does not match model",
                    ))?;
                let entry = copies.entry(std::ptr::from_ref(model).addr());
                let position = entry.index();
                entry.or_insert_with(|| T::from_state(model.state().clone()));
                parents.push((index, position, target.clone()));
            }
        }
        Ok(Self {
            models: copies.into_values().collect(),
            parents,
        })
    }

    fn install<M: Model>(self, models: &mut [M], name: &str) {
        let replacements: Vec<_> = self.models.into_iter().map(Arc::new).collect();
        for (index, position, mut target) in self.parents {
            target.replace_model(replacements[position].clone());
            let model = &mut models[index];
            model.unsetRelation(name);
            model
                .state_mut()
                .relations
                .insert_relation::<MorphTo<M>>(name.to_owned(), Some(target));
        }
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

fn model_dictionary<T>(
    models: impl IntoIterator<Item = T>,
    key_of: impl Fn(&T) -> Option<&Value>,
) -> DatabaseResult<IndexMap<Vec<u8>, T>> {
    let mut dictionary = IndexMap::new();
    for model in models {
        if let Some(key) = dictionary_key(key_of(&model))? {
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

    fn into_iter(mut self) -> Self::IntoIter {
        if self.autoload {
            super::detached_autoload::attach(&mut self.models);
        }
        self.models.into_iter()
    }
}
