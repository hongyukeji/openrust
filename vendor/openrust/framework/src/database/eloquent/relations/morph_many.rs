use std::sync::Arc;

use crate::database::{
    Connection, DatabaseFuture, DatabaseResult,
    eloquent::{Attributes, Builder, Model},
    query,
};

use super::{ExistenceRelation, HasMany, MorphOne, Relation};

/// Links a parent model to many related records tagged with a morph alias.
pub struct MorphMany<P: Model, R: Model>(HasMany<P, R>);

impl<P: Model, R: Model> MorphMany<P, R> {
    pub const fn new(
        local_key: &'static str,
        id_column: &'static str,
        type_column: &'static str,
        alias: &'static str,
    ) -> Self {
        Self(HasMany::morph(local_key, id_column, type_column, alias))
    }

    pub fn one(self) -> MorphOne<P, R> {
        MorphOne(self.0.one())
    }

    #[must_use]
    pub fn constrain(
        self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        Self(self.0.constrain(configure))
    }

    pub async fn save(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        self.0.save(parent, model).await
    }

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.create(parent, attributes).await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model, R: Model> Relation for MorphMany<P, R> {
    type Parent = P;
    type Output = Vec<Arc<R>>;

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.0.matchResults(parents)
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
