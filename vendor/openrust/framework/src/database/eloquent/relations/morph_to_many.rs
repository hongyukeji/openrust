use std::ops::Deref;

use crate::database::{
    Connection, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Builder, Model},
    query,
};

use super::{BelongsToMany, ExistenceRelation, Relation, pivot_constraints::pivot_predicates};

/// Links two models through an intermediate table with a morph discriminator.
pub struct MorphToMany<P: Model, R: Model>(BelongsToMany<P, R>);

impl<P: Model, R: Model> MorphToMany<P, R> {
    pub const fn new(
        table: &'static str,
        foreign_pivot_key: &'static str,
        related_pivot_key: &'static str,
        parent_key: &'static str,
        related_key: &'static str,
        type_column: &'static str,
        alias: &'static str,
    ) -> Self {
        Self(
            BelongsToMany::new(
                table,
                foreign_pivot_key,
                related_pivot_key,
                parent_key,
                related_key,
            )
            .morph(type_column, alias),
        )
    }

    /// Builds the descriptor using the morph owner's application alias.
    pub fn mapped(
        table: &'static str,
        foreign_pivot_key: &'static str,
        related_pivot_key: &'static str,
        parent_key: &'static str,
        related_key: &'static str,
        type_column: &'static str,
        owner: &impl Model,
    ) -> DatabaseResult<Self> {
        Ok(Self::new(
            table,
            foreign_pivot_key,
            related_pivot_key,
            parent_key,
            related_key,
            type_column,
            owner.get_morph_class()?,
        ))
    }

    #[must_use]
    pub fn constrain(
        self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        Self(self.0.constrain(configure))
    }

    #[must_use]
    pub fn with_pivot(self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        Self(self.0.with_pivot(columns))
    }

    #[must_use]
    pub fn order_by_pivot(self, column: &str, direction: &str) -> Self {
        Self(self.0.order_by_pivot(column, direction))
    }

    #[must_use]
    pub fn order_by_pivot_desc(self, column: &str) -> Self {
        Self(self.0.order_by_pivot_desc(column))
    }

    #[must_use]
    pub fn with_timestamps(self) -> Self {
        Self(self.0.with_timestamps())
    }

    pub fn with_timestamp_columns(
        self,
        created: Option<&'static str>,
        updated: Option<&'static str>,
    ) -> DatabaseResult<Self> {
        self.0.with_timestamp_columns(created, updated).map(Self)
    }

    #[must_use]
    pub fn using<M: Model>(self) -> Self {
        Self(self.0.using::<M>())
    }

    pub fn chaperone(self) -> Self {
        Self(self.0.chaperone())
    }

    pub fn chaperone_with(
        self,
        declaring: Option<&str>,
        related: Option<&str>,
    ) -> DatabaseResult<Self> {
        self.0.chaperone_with(declaring, related).map(Self)
    }

    #[must_use]
    pub fn without_chaperone(self) -> Self {
        Self(self.0.without_chaperone())
    }

    #[must_use]
    pub fn with_touching_parent(self) -> Self {
        Self(self.0.with_touching_parent())
    }

    #[must_use]
    pub fn with_touching_related(self) -> Self {
        Self(self.0.with_touching_related())
    }

    pub(in crate::database::eloquent) fn into_inner(self) -> BelongsToMany<P, R> {
        self.0
    }

    pivot_predicates!(apply_pivot_predicate, apply_pivot_group);

    fn apply_pivot_predicate(
        self,
        column: &str,
        predicate: impl FnOnce(query::Conditions, &str) -> query::Conditions,
    ) -> Self {
        Self(self.0.apply_pivot_predicate(column, predicate))
    }

    fn apply_pivot_group(
        self,
        or: bool,
        configure: impl FnOnce(query::Conditions) -> query::Conditions,
    ) -> Self {
        Self(self.0.apply_pivot_group(or, configure))
    }

    /// Filters the relation and supplies the value when inserting new links.
    pub fn with_pivot_value(self, column: &str, value: impl Into<Value>) -> DatabaseResult<Self> {
        self.0.with_pivot_value(column, value).map(Self)
    }

    #[must_use]
    pub fn r#as(self, name: &'static str) -> Self {
        Self(self.0.r#as(name))
    }
}

impl<P: Model, R: Model> From<MorphToMany<P, R>> for BelongsToMany<P, R> {
    fn from(relation: MorphToMany<P, R>) -> Self {
        relation.0
    }
}

impl<P: Model, R: Model> Deref for MorphToMany<P, R> {
    type Target = BelongsToMany<P, R>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<P: Model, R: Model> Relation for MorphToMany<P, R> {
    type Parent = P;
    type Output = <BelongsToMany<P, R> as Relation>::Output;

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
        super::super::queueable::common_relations(output.iter().map(|model| model.as_ref()))
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

impl<P: Model, R: Model> MorphToMany<P, R> {
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

impl<P: Model, R: Model> ExistenceRelation for MorphToMany<P, R> {
    fn get_relation_existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0
            .get_relation_existence_query(connection, parent_table)
    }
}

impl<P: Model, R: Model> super::NestedRelation for MorphToMany<P, R> {
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

impl<P: Model, R: Model> super::ConstrainableRelation for MorphToMany<P, R> {
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
