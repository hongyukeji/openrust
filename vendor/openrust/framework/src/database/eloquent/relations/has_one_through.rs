use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult,
    eloquent::{Attributes, Builder, Model},
    query,
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

#[allow(non_snake_case, reason = "Matches Laravel's through relation API")]
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
    pub fn withDefault(mut self, configure: impl Fn(&mut R, &P) + Send + Sync + 'static) -> Self {
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

    #[must_use]
    pub fn latestOfMany(self, column: Option<&'static str>) -> Self {
        self.ofMany(&[(
            column.unwrap_or(R::metadata().primary_key),
            OneOfManyAggregate::Max,
        )])
    }

    #[must_use]
    pub fn oldestOfMany(self, column: Option<&'static str>) -> Self {
        self.ofMany(&[(
            column.unwrap_or(R::metadata().primary_key),
            OneOfManyAggregate::Min,
        )])
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

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model, I: Model, R: Model> Relation for HasOneThrough<P, I, R> {
    type Parent = P;
    type Output = Option<Arc<R>>;

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(async move {
            let matches = self.0.matchResults(parents).await?;
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

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, I: Model, R: Model> ExistenceRelation for HasOneThrough<P, I, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.getRelationExistenceQuery(connection, parent_table)
    }
}
