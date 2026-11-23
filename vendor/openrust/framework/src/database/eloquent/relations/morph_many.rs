use std::sync::Arc;

use crate::database::{
    Connection, DatabaseCursor, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use crate::pagination::{
    CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, Paginator,
};

use super::RelatedValues;

use super::{ExistenceRelation, HasMany, MorphOne, Relation};

/// Links a parent model to many related records tagged with a morph alias.
pub struct MorphMany<P: Model, R: Model>(pub(super) HasMany<P, R>);

impl<P: Model, R: Model> MorphMany<P, R> {
    pub async fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.0.paginate(parent, per_page).await
    }

    pub async fn paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.0.paginate_with(parent, options).await
    }

    pub async fn simple_paginate(&self, parent: &P, per_page: u64) -> DatabaseResult<Paginator<R>> {
        self.0.simple_paginate(parent, per_page).await
    }

    pub async fn simple_paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<Paginator<R>> {
        self.0.simple_paginate_with(parent, options).await
    }

    pub async fn cursor_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.0.cursor_paginate(parent, per_page).await
    }

    pub async fn cursor_paginate_with(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.0.cursor_paginate_with(parent, options).await
    }
}

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
            parent.get_morph_class()?,
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
    pub fn without_chaperone(self) -> Self {
        Self(self.0.without_chaperone())
    }

    #[must_use]
    pub fn without_inverse(self) -> Self {
        self.without_chaperone()
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
    pub async fn make_many(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        self.0.make_many(parent, records).await
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
    super::key_chunks::lazy_chunks!(forward 0);

    pub fn cursor<'a>(&'a self, parent: &'a P) -> DatabaseCursor<'a, R> {
        self.0.cursor(parent)
    }

    pub async fn find_or_new(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        self.0.find_or_new(parent, key).await
    }

    super::lookup::relation_lookup!(.0);

    pub async fn first_or_new<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.first_or_new(parent, attributes, values).await
    }

    pub async fn first_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.first_or_create(parent, attributes, values).await
    }

    pub async fn create_or_first<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.create_or_first(parent, attributes, values).await
    }

    pub async fn update_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.update_or_create(parent, attributes, values).await
    }

    pub async fn restore_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.restore_or_create(parent, attributes, values).await
    }

    pub async fn create_or_restore<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.0.create_or_restore(parent, attributes, values).await
    }

    pub async fn increment_or_create(
        &self,
        parent: &P,
        attributes: Attributes,
        column: &str,
        default: impl Into<Value>,
        step: impl Into<Value>,
        extra: Attributes,
    ) -> DatabaseResult<R> {
        self.0
            .increment_or_create(parent, attributes, column, default, step, extra)
            .await
    }

    pub async fn save_quietly(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        self.0.save_quietly(parent, model).await
    }

    pub async fn save_many(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        self.0.save_many(parent, models).await
    }

    pub async fn save_many_quietly(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        self.0.save_many_quietly(parent, models).await
    }

    pub async fn create_quietly(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.create_quietly(parent, attributes).await
    }

    pub async fn create_many(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.create_many(parent, records).await
    }

    pub async fn create_many_quietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.create_many_quietly(parent, records).await
    }

    pub async fn force_create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.force_create(parent, attributes).await
    }

    pub async fn force_create_quietly(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<R> {
        self.0.force_create_quietly(parent, attributes).await
    }

    pub async fn force_create_many(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.force_create_many(parent, records).await
    }

    pub async fn force_create_many_quietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        self.0.force_create_many_quietly(parent, records).await
    }

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.create(parent, attributes).await
    }
}

impl<P: Model, R: Model> Relation for MorphMany<P, R> {
    type Parent = P;
    type Output = super::super::RelatedCollection<R>;

    fn is_empty_collection(output: &Self::Output) -> bool {
        output.is_empty()
    }

    fn attach_autoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn push_loaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(async move {
            let (models, result) = context.push_many(output).await;
            (models.map(|models| output.with_models(models)), result)
        })
    }

    fn autoload_output(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::many(output)
    }

    fn queueable_relations(output: &Self::Output) -> Vec<String> {
        super::super::queueable::common_relations(output.iter().map(Arc::as_ref))
    }

    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.match_results(parents, None)
    }

    fn match_results_in_transaction<'a>(
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
            Some(tx) => self.0.match_results_in_transaction(parents, tx),
            None => self.0.match_results(parents),
        }
    }
}

impl<P: Model, R: Model> ExistenceRelation for MorphMany<P, R> {
    fn get_relation_existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0
            .get_relation_existence_query(connection, parent_table)
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
