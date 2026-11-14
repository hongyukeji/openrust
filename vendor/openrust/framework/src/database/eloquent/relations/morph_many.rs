use std::sync::Arc;

use crate::database::{
    Connection, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use super::RelatedValues;

use super::{ExistenceRelation, HasMany, MorphOne, Relation};

/// Links a parent model to many related records tagged with a morph alias.
pub struct MorphMany<P: Model, R: Model>(pub(super) HasMany<P, R>);

impl<P: Model, R: Model> MorphMany<P, R> {
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
        Self(HasMany::morph(local_key, id_column, type_column, alias))
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn with_exact_morph_alias(mut self, alias: &'static str) -> Self {
        self.0.0.morph_constraint = Some(Box::new(move |query| {
            Ok(query.require_mysql_morph_alias(alias))
        }));
        self
    }

    pub fn one(self) -> MorphOne<P, R> {
        MorphOne(self.0.one())
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
    #[allow(non_snake_case, reason = "Matches Laravel's inverse relationship API")]
    pub fn withoutChaperone(self) -> Self {
        Self(self.0.withoutChaperone())
    }

    #[must_use]
    #[allow(non_snake_case, reason = "Matches Laravel's inverse relationship API")]
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
    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn makeMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        self.0.makeMany(parent, records).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn firstOrNew<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.firstOrNew(parent, attributes, values).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn firstOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.firstOrCreate(parent, attributes, values).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn createOrFirst<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.createOrFirst(parent, attributes, values).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn updateOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.updateOrCreate(parent, attributes, values).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn restoreOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.restoreOrCreate(parent, attributes, values).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn createOrRestore<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.createOrRestore(parent, attributes, values).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
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

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn saveQuietly(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        self.0.saveQuietly(parent, model).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn saveMany(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        self.0.saveMany(parent, models).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn saveManyQuietly(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        self.0.saveManyQuietly(parent, models).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn createQuietly(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.createQuietly(parent, attributes).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn createMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.createMany(parent, records).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn createManyQuietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.createManyQuietly(parent, records).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
    pub async fn forceCreate(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.forceCreate(parent, attributes).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
    pub async fn forceCreateQuietly(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<R> {
        self.0.forceCreateQuietly(parent, attributes).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
    pub async fn forceCreateMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.forceCreateMany(parent, records).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
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

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model, R: Model> Relation for MorphMany<P, R> {
    type Parent = P;
    type Output = super::super::RelatedCollection<R>;

    fn attachAutoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn pushLoaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(async move {
            let (models, result) = context.push_many(output).await;
            (models.map(|models| output.with_models(models)), result)
        })
    }

    fn autoloadOutput(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::many(output)
    }

    fn queueableRelations(output: &Self::Output) -> Vec<String> {
        super::super::queueable::common_relations(output.iter().map(Arc::as_ref))
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

impl<P: Model, R: Model> MorphMany<P, R> {
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

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, R: Model> ExistenceRelation for MorphMany<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.getRelationExistenceQuery(connection, parent_table)
    }
}

impl<P: Model, R: Model> super::NestedRelation for MorphMany<P, R> {
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

impl<P: Model, R: Model> super::ConstrainableRelation for MorphMany<P, R> {
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
