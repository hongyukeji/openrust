use std::ops::Deref;

use crate::database::{
    Connection, DatabaseFuture, DatabaseResult,
    eloquent::{Builder, Model},
    query,
};

use super::{BelongsToMany, ExistenceRelation, Relation};

/// Links two models through an intermediate table with a morph discriminator.
pub struct MorphToMany<P: Model, R: Model>(BelongsToMany<P, R>);

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
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

    #[must_use]
    pub fn constrain(
        self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        Self(self.0.constrain(configure))
    }

    #[must_use]
    pub fn withPivot(self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        Self(self.0.withPivot(columns))
    }

    #[must_use]
    pub fn r#as(self, name: &'static str) -> Self {
        Self(self.0.r#as(name))
    }
}

impl<P: Model, R: Model> Deref for MorphToMany<P, R> {
    type Target = BelongsToMany<P, R>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model, R: Model> Relation for MorphToMany<P, R> {
    type Parent = P;
    type Output = <BelongsToMany<P, R> as Relation>::Output;

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.0.matchResults(parents)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, R: Model> ExistenceRelation for MorphToMany<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.getRelationExistenceQuery(connection, parent_table)
    }
}
