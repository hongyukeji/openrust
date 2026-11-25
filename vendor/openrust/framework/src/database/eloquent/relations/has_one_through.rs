use std::sync::Arc;

use crate::database::{
    Connection, DatabaseCursor, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use crate::pagination::{
    CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, Paginator,
};

use super::{
    ExistenceRelation, HasManyThrough, OneOfManyAggregate, Relation,
    default_model::{ConfigureDefault, match_defaults},
    one_of_many::OneOfMany,
};

/// Links a parent to one related model through an intermediate model.
pub struct HasOneThrough<P: Model, I: Model, R: Model>(
    HasManyThrough<P, I, R>,
    Option<Box<ConfigureDefault<P, R>>>,
);

impl<P: Model, I: Model, R: Model> HasOneThrough<P, I, R> {
    pub const fn new(
        first_key: &'static str,
        second_key: &'static str,
        local_key: &'static str,
        second_local_key: &'static str,
    ) -> Self {
        Self::from_many(HasManyThrough::new(
            first_key,
            second_key,
            local_key,
            second_local_key,
        ))
    }

    pub(super) const fn from_many(relation: HasManyThrough<P, I, R>) -> Self {
        Self(relation, None)
    }

    /// Configures an unsaved related model when the through relation is missing.
    #[must_use]
    pub fn with_default(mut self, configure: impl Fn(&mut R, &P) + Send + Sync + 'static) -> Self {
        self.1 = Some(Box::new(configure));
        self
    }

    #[must_use]
    pub fn with_trashed_parents(mut self) -> Self {
        self.0 = self.0.with_trashed_parents();
        self
    }

    #[must_use]
    pub fn constrain(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.0 = self.0.constrain(configure);
        self
    }

    #[track_caller]
    pub fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<R>>> {
        self.0.paginate(parent, per_page)
    }

    #[track_caller]
    pub fn paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<R>>> {
        self.0.paginate_with(parent, options)
    }

    #[track_caller]
    pub fn simple_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<Paginator<R>>> {
        self.0.simple_paginate(parent, per_page)
    }

    #[track_caller]
    pub fn simple_paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<Paginator<R>>> {
        self.0.simple_paginate_with(parent, options)
    }

    #[track_caller]
    pub fn cursor_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<R>>> {
        self.0.cursor_paginate(parent, per_page)
    }

    #[track_caller]
    pub fn cursor_paginate_with(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<R>>> {
        self.0.cursor_paginate_with(parent, options)
    }

    pub async fn is(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        match related {
            Some(related) => self.0.contains_model(parent, related).await,
            None => Ok(false),
        }
    }

    pub async fn is_not(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        Ok(!self.is(parent, related).await?)
    }

    query_callers! {
    /// Writes the related table using the supplied intermediate foreign keys.
    pub async fn upsert(
        &self,
        parent: &P,
        rows: Vec<Attributes>,
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        self.0.upsert(parent, rows, unique_by, update).await
    }
    }

    /// Reads this parent's related models in ordered batches.
    #[track_caller]
    pub fn chunk<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
    ) -> impl Future<Output = DatabaseResult<bool>>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.0.chunk(parent, count, callback)
    }

    super::key_chunks::key_chunks!(forward 0);
    super::key_chunks::lazy_chunks!(forward 0);

    #[track_caller]
    pub fn cursor<'a>(&'a self, parent: &'a P) -> DatabaseCursor<'a, R> {
        self.0.cursor(parent)
    }

    query_callers! {
    pub async fn find_or_new(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        self.0.find_or_new(parent, key).await
    }
    }

    super::lookup::relation_lookup!(.0);

    query_callers! {
    pub async fn first_or_new(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.first_or_new(parent, attributes, values).await
    }

    pub async fn first_or_create(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.first_or_create(parent, attributes, values).await
    }

    pub async fn create_or_first(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.create_or_first(parent, attributes, values).await
    }

    pub async fn update_or_create(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.update_or_create(parent, attributes, values).await
    }

    pub async fn restore_or_create(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.restore_or_create(parent, attributes, values).await
    }

    pub async fn create_or_restore(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
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
    }

    #[must_use]
    pub fn latest_of_many<'a>(mut self, columns: impl Into<super::OneOfManyColumns<'a>>) -> Self {
        self.0.selection = Some(OneOfMany::shortcut::<R>(
            columns.into(),
            OneOfManyAggregate::Max,
        ));
        self
    }

    #[must_use]
    pub fn oldest_of_many<'a>(mut self, columns: impl Into<super::OneOfManyColumns<'a>>) -> Self {
        self.0.selection = Some(OneOfMany::shortcut::<R>(
            columns.into(),
            OneOfManyAggregate::Min,
        ));
        self
    }

    #[must_use]
    pub fn of_many(mut self, columns: &[(&'static str, OneOfManyAggregate)]) -> Self {
        self.0.selection = Some(OneOfMany::new::<R>(
            columns.iter().copied(),
            OneOfManyAggregate::Max,
        ));
        self
    }

    #[must_use]
    pub fn of_many_by(mut self, column: &'static str, aggregate: OneOfManyAggregate) -> Self {
        self.0.selection = Some(OneOfMany::new::<R>([(column, aggregate)], aggregate));
        self
    }

    pub fn where_candidates(
        mut self,
        filter: impl Fn(query::Conditions) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        self.0
            .selection
            .as_mut()
            .ok_or(DatabaseError::InvalidQuery(
                "configure one-of-many before its candidates",
            ))?
            .filter(filter);
        Ok(self)
    }
}

impl<P: Model, I: Model, R: Model> Relation for HasOneThrough<P, I, R> {
    type Parent = P;
    type Output = Option<Arc<R>>;

    fn attach_autoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn push_loaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(context.push_one(output))
    }

    fn autoload_output(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::one(output)
    }

    fn queueable_relations(output: &Self::Output) -> Vec<String> {
        output
            .as_deref()
            .map_or_else(Vec::new, Model::get_queueable_relations)
    }

    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, None),
        ))
    }

    fn match_results_in_transaction<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, Some(transaction)),
        ))
    }
}

impl<P: Model, I: Model, R: Model> HasOneThrough<P, I, R> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        Box::pin(async move {
            let matches = match transaction {
                Some(tx) => self.0.match_results_in_transaction(parents, tx),
                None => self.0.match_results(parents),
            }
            .await?;
            match_defaults(
                parents,
                matches.into_iter().map(|models| models.into_iter().next()),
                self.1.as_deref(),
                &self.0.keys,
                |_| Attributes::new(),
            )
        })
    }
}

impl<P: Model, I: Model, R: Model> ExistenceRelation for HasOneThrough<P, I, R> {
    fn get_relation_existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0
            .get_relation_existence_query(connection, parent_table)
    }
}

impl<P: Model, I: Model, R: Model> super::NestedRelation for HasOneThrough<P, I, R> {
    type Related = R;

    fn with_nested(mut self, nested: super::super::EagerLoads<R>) -> Self {
        self.0.keys.with_nested(nested);
        self
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

impl<P: Model, I: Model, R: Model> super::ConstrainableRelation for HasOneThrough<P, I, R> {
    type Related = R;

    fn constrain_query(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.0.keys.append_constraint(configure);
        self
    }
}
