use std::sync::Arc;

use crate::database::{
    Connection, DatabaseCursor, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use super::RelatedValues;

use super::{ExistenceRelation, HasMany, HasOne, OneOfManyAggregate, Relation};

/// Links a parent model to one related record tagged with a morph alias.
pub struct MorphOne<P: Model, R: Model>(pub(super) HasOne<P, R>);

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<P: Model, R: Model> MorphOne<P, R> {
    /// Configures an unsaved model for a missing polymorphic match.
    #[must_use]
    pub fn withDefault(self, configure: impl Fn(&mut R, &P) + Send + Sync + 'static) -> Self {
        Self(self.0.withDefault(configure))
    }

    /// Builds the descriptor using the parent's application morph map.
    pub fn mapped(
        parent: &P,
        local_key: &'static str,
        id_column: &'static str,
        type_column: &'static str,
    ) -> DatabaseResult<Self> {
        Ok(Self::new(
            local_key,
            id_column,
            type_column,
            parent.getMorphClass()?,
        ))
    }

    pub const fn new(
        local_key: &'static str,
        id_column: &'static str,
        type_column: &'static str,
        alias: &'static str,
    ) -> Self {
        Self(HasMany::morph(local_key, id_column, type_column, alias).one())
    }

    /// Hydrates the named inverse relation from the parent already in memory.
    #[must_use]
    pub fn chaperone(self, relation: &'static str) -> Self {
        Self(self.0.chaperone(relation))
    }

    #[must_use]
    pub fn inverse(self, relation: &'static str) -> Self {
        self.chaperone(relation)
    }

    #[must_use]
    pub fn withoutChaperone(self) -> Self {
        Self(self.0.withoutChaperone())
    }

    #[must_use]
    pub fn withoutInverse(self) -> Self {
        self.withoutChaperone()
    }

    #[must_use]
    pub fn constrain(
        self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        Self(self.0.constrain(configure))
    }

    pub async fn is(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        match related {
            Some(related) => self.0.is(parent, Some(related)).await,
            None => Ok(false),
        }
    }

    pub async fn isNot(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        Ok(!self.is(parent, related).await?)
    }

    #[must_use]
    pub fn latestOfMany<'a>(self, columns: impl Into<super::OneOfManyColumns<'a>>) -> Self {
        Self(self.0.latestOfMany(columns))
    }

    #[must_use]
    pub fn oldestOfMany<'a>(self, columns: impl Into<super::OneOfManyColumns<'a>>) -> Self {
        Self(self.0.oldestOfMany(columns))
    }

    #[must_use]
    pub fn ofMany(self, columns: &[(&'static str, OneOfManyAggregate)]) -> Self {
        Self(self.0.ofMany(columns))
    }

    #[must_use]
    pub fn ofManyBy(self, column: &'static str, aggregate: OneOfManyAggregate) -> Self {
        Self(self.0.ofManyBy(column, aggregate))
    }

    pub fn whereCandidates(
        self,
        filter: impl Fn(query::Conditions) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        Ok(Self(self.0.whereCandidates(filter)?))
    }

    /// Writes related rows, assigning the parent key and any morph discriminator.
    pub async fn upsert(
        &self,
        parent: &P,
        rows: Vec<Attributes>,
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        self.0.upsert(parent, rows, unique_by, update).await
    }

    pub(in crate::database::eloquent) fn factory_attributes(
        &self,
        parent: &P,
    ) -> DatabaseResult<(Attributes, Attributes)> {
        self.0.factory_attributes(parent)
    }

    pub(in crate::database::eloquent) async fn save_factory_child(
        &self,
        parent: &P,
        model: &mut R,
    ) -> DatabaseResult<bool> {
        self.0.save_factory_child(parent, model).await
    }

    pub async fn save(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        self.0.save(parent, model).await
    }

    /// Constructs an unsaved child with the relationship attributes.
    pub async fn make(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.make(parent, attributes).await
    }

    /// Constructs unsaved children in input order.
    pub async fn makeMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        self.0.makeMany(parent, records).await
    }

    /// Reads this parent's related models in ordered batches.
    pub async fn chunk<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.0.chunk(parent, count, callback).await
    }

    super::key_chunks::key_chunks!(forward 0);
    super::pagination::relation_pages!();

    fn pagination_target(&self, parent: &P) -> DatabaseResult<Option<Builder<'static, R>>> {
        self.0.pagination_target(parent)
    }

    super::key_chunks::lazy_chunks!(forward 0);

    pub fn cursor<'a>(&'a self, parent: &'a P) -> DatabaseCursor<'a, R> {
        self.0.cursor(parent)
    }

    pub async fn findOrNew(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        self.0.findOrNew(parent, key).await
    }

    super::lookup::relation_lookup!(.0.0);

    pub async fn firstOrNew<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.firstOrNew(parent, attributes, values).await
    }

    pub async fn firstOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.firstOrCreate(parent, attributes, values).await
    }

    pub async fn createOrFirst<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.createOrFirst(parent, attributes, values).await
    }

    pub async fn updateOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.updateOrCreate(parent, attributes, values).await
    }

    pub async fn restoreOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.restoreOrCreate(parent, attributes, values).await
    }

    pub async fn createOrRestore<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.createOrRestore(parent, attributes, values).await
    }

    pub async fn incrementOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        column: &str,
        default: impl Into<Value>,
        step: impl Into<Value>,
        extra: Attributes,
    ) -> DatabaseResult<R> {
        self.0
            .incrementOrCreate(parent, attributes, column, default, step, extra)
            .await
    }

    pub async fn saveQuietly(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        self.0.saveQuietly(parent, model).await
    }

    pub async fn saveMany(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        self.0.saveMany(parent, models).await
    }

    pub async fn saveManyQuietly(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        self.0.saveManyQuietly(parent, models).await
    }

    pub async fn createQuietly(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.createQuietly(parent, attributes).await
    }

    pub async fn createMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.createMany(parent, records).await
    }

    pub async fn createManyQuietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.createManyQuietly(parent, records).await
    }

    pub async fn forceCreate(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.forceCreate(parent, attributes).await
    }

    pub async fn forceCreateQuietly(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<R> {
        self.0.forceCreateQuietly(parent, attributes).await
    }

    pub async fn forceCreateMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.forceCreateMany(parent, records).await
    }

    pub async fn forceCreateManyQuietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.forceCreateManyQuietly(parent, records).await
    }

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.create(parent, attributes).await
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<P: Model, R: Model> Relation for MorphOne<P, R> {
    type Parent = P;
    type Output = Option<Arc<R>>;

    fn attachAutoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn pushLoaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(context.push_one(output))
    }

    fn autoloadOutput(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::one(output)
    }

    fn queueableRelations(output: &Self::Output) -> Vec<String> {
        output
            .as_deref()
            .map_or_else(Vec::new, Model::getQueueableRelations)
    }

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.match_results(parents, None)
    }

    fn matchResultsInTransaction<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.match_results(parents, Some(transaction))
    }
}

impl<P: Model, R: Model> MorphOne<P, R> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        match transaction {
            Some(tx) => self.0.matchResultsInTransaction(parents, tx),
            None => self.0.matchResults(parents),
        }
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<P: Model, R: Model> ExistenceRelation for MorphOne<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.getRelationExistenceQuery(connection, parent_table)
    }
}

impl<P: Model, R: Model> super::NestedRelation for MorphOne<P, R> {
    type Related = R;

    fn with_nested(self, nested: super::super::EagerLoads<R>) -> Self {
        Self(super::NestedRelation::with_nested(self.0, nested))
    }

    fn with_path_columns(mut self, columns: Vec<String>) -> DatabaseResult<Self> {
        self.0 = super::NestedRelation::with_path_columns(self.0, columns)?;
        Ok(self)
    }

    fn with_path_constraint(
        self,
        constraint: impl Fn(
            crate::database::query::Conditions,
            &str,
        ) -> crate::database::query::Conditions
        + Send
        + Sync
        + 'static,
    ) -> DatabaseResult<Self> {
        Ok(super::ConstrainableRelation::constrain_query(
            self,
            move |query| query.with_path_constraint(&constraint),
        ))
    }
}

impl<P: Model, R: Model> super::ConstrainableRelation for MorphOne<P, R> {
    type Related = R;

    fn constrain_query(
        self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        Self(super::ConstrainableRelation::constrain_query(
            self.0, configure,
        ))
    }
}
