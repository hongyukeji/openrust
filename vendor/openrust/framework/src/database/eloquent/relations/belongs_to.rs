use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use super::{
    ExistenceRelation, Relation,
    default_model::{ConfigureDefault, match_defaults},
    relation::RelationKeys,
};

/// Resolves a model's foreign key against a related owner's key.
pub struct BelongsTo<P: Model, R: Model>(RelationKeys<P, R>, Option<Box<ConfigureDefault<P, R>>>);

#[allow(non_snake_case, reason = "Matches Laravel's default relationship API")]
impl<P: Model, R: Model> BelongsTo<P, R> {
    pub(in crate::database::eloquent) fn identity_columns(&self) -> (&str, &str) {
        (self.0.parent, self.0.related)
    }

    pub const fn new(foreign_key: &'static str, owner_key: &'static str) -> Self {
        Self(RelationKeys::new(foreign_key, owner_key), None)
    }

    /// Configures an unsaved owner for children without a matching owner.
    #[must_use]
    pub fn withDefault(mut self, configure: impl Fn(&mut R, &P) + Send + Sync + 'static) -> Self {
        self.1 = Some(Box::new(configure));
        self
    }

    /// Applies constraints and nested eager loads to the related query.
    #[must_use]
    pub fn constrain(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.0.constrain(configure);
        self
    }

    pub fn associate(&self, model: &mut P, related: &R) -> DatabaseResult<()> {
        let value = related
            .getAttribute(self.0.related)
            .filter(|value| !matches!(value, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship owner key"))?;
        if let (Some(parent), Some(owner)) =
            (&model.state().connection, &related.state().connection)
            && !parent.same_query_source(owner)
            && R::metadata().connection.is_none()
        {
            return Err(DatabaseError::InvalidQuery(
                "mixed relationship connections",
            ));
        }
        model.setAttribute(self.0.parent, value.clone());
        model.state_mut().relations.clear();
        Ok(())
    }

    pub fn dissociate(&self, model: &mut P) {
        model.setAttribute(self.0.parent, Value::Null);
        model.state_mut().relations.clear();
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> Relation for BelongsTo<P, R> {
    type Parent = P;
    type Output = Option<Arc<R>>;

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(async move {
            let matches = self.0.match_many(parents).await?;
            match_defaults(
                parents,
                matches.into_iter().map(|rows| rows.into_iter().last()),
                self.1.as_deref(),
                &self.0,
                |_| Attributes::new(),
            )
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, R: Model> ExistenceRelation for BelongsTo<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.existence_query(connection, parent_table)
    }
}
