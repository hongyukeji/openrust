use std::sync::Arc;

use crate::database::{
    Connection, DatabaseFuture, DatabaseResult,
    eloquent::{Attributes, Builder, Model},
    query,
};

use super::{ExistenceRelation, HasMany, HasOne, OneOfManyAggregate, Relation};

/// Links a parent model to one related record tagged with a morph alias.
pub struct MorphOne<P: Model, R: Model>(pub(super) HasOne<P, R>);

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> MorphOne<P, R> {
    /// Configures an unsaved model for a missing polymorphic match.
    #[must_use]
    pub fn withDefault(self, configure: impl Fn(&mut R, &P) + Send + Sync + 'static) -> Self {
        Self(self.0.withDefault(configure))
    }

    pub const fn new(
        local_key: &'static str,
        id_column: &'static str,
        type_column: &'static str,
        alias: &'static str,
    ) -> Self {
        Self(HasMany::morph(local_key, id_column, type_column, alias).one())
    }

    #[must_use]
    pub fn constrain(
        self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        Self(self.0.constrain(configure))
    }

    #[must_use]
    pub fn latestOfMany(self, column: Option<&'static str>) -> Self {
        Self(self.0.latestOfMany(column))
    }

    #[must_use]
    pub fn oldestOfMany(self, column: Option<&'static str>) -> Self {
        Self(self.0.oldestOfMany(column))
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

    pub async fn save(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        self.0.save(parent, model).await
    }

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.create(parent, attributes).await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model, R: Model> Relation for MorphOne<P, R> {
    type Parent = P;
    type Output = Option<Arc<R>>;

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.0.matchResults(parents)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, R: Model> ExistenceRelation for MorphOne<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.getRelationExistenceQuery(connection, parent_table)
    }
}
