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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
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
    pub fn withTrashedParents(mut self) -> Self {
        self.0 = self.0.withTrashedParents();
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

    pub async fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.0.paginate(parent, per_page).await
    }

    pub async fn paginateWith(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.0.paginateWith(parent, options).await
    }

    pub async fn simplePaginate(&self, parent: &P, per_page: u64) -> DatabaseResult<Paginator<R>> {
        self.0.simplePaginate(parent, per_page).await
    }

    pub async fn simplePaginateWith(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<Paginator<R>> {
        self.0.simplePaginateWith(parent, options).await
    }

    pub async fn cursorPaginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.0.cursorPaginate(parent, per_page).await
    }

    pub async fn cursorPaginateWith(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.0.cursorPaginateWith(parent, options).await
    }

    pub async fn is(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        match related {
            Some(related) => self.0.contains_model(parent, related).await,
            None => Ok(false),
        }
    }

    pub async fn isNot(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        Ok(!self.is(parent, related).await?)
    }

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

    pub async fn findOrNew(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        self.0.findOrNew(parent, key).await
    }

    super::lookup::relation_lookup!(.0);

    pub async fn firstOrNew(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.firstOrNew(parent, attributes, values).await
    }

    pub async fn firstOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.firstOrCreate(parent, attributes, values).await
    }

    pub async fn createOrFirst(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.createOrFirst(parent, attributes, values).await
    }

    pub async fn updateOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.updateOrCreate(parent, attributes, values).await
    }

    pub async fn restoreOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.0.restoreOrCreate(parent, attributes, values).await
    }

    pub async fn createOrRestore(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
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

    #[must_use]
    pub fn latestOfMany<'a>(mut self, columns: impl Into<super::OneOfManyColumns<'a>>) -> Self {
        self.0.selection = Some(OneOfMany::shortcut::<R>(
            columns.into(),
            OneOfManyAggregate::Max,
        ));
        self
    }

    #[must_use]
    pub fn oldestOfMany<'a>(mut self, columns: impl Into<super::OneOfManyColumns<'a>>) -> Self {
        self.0.selection = Some(OneOfMany::shortcut::<R>(
            columns.into(),
            OneOfManyAggregate::Min,
        ));
        self
    }

    #[must_use]
    pub fn ofMany(mut self, columns: &[(&'static str, OneOfManyAggregate)]) -> Self {
        self.0.selection = Some(OneOfMany::new::<R>(
            columns.iter().copied(),
            OneOfManyAggregate::Max,
        ));
        self
    }

    #[must_use]
    pub fn ofManyBy(mut self, column: &'static str, aggregate: OneOfManyAggregate) -> Self {
        self.0.selection = Some(OneOfMany::new::<R>([(column, aggregate)], aggregate));
        self
    }

    pub fn whereCandidates(
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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<P: Model, I: Model, R: Model> Relation for HasOneThrough<P, I, R> {
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

impl<P: Model, I: Model, R: Model> HasOneThrough<P, I, R> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        Box::pin(async move {
            let matches = match transaction {
                Some(tx) => self.0.matchResultsInTransaction(parents, tx),
                None => self.0.matchResults(parents),
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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<P: Model, I: Model, R: Model> ExistenceRelation for HasOneThrough<P, I, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.getRelationExistenceQuery(connection, parent_table)
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
