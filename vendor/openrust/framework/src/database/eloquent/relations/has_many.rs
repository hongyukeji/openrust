use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use super::{ExistenceRelation, HasOne, Relation, relation::RelationKeys};

/// Links a parent key to a foreign key on many related models.
pub struct HasMany<P: Model, R: Model>(pub(super) RelationKeys<P, R>);

impl<P: Model, R: Model> HasMany<P, R> {
    pub const fn new(local_key: &'static str, foreign_key: &'static str) -> Self {
        Self(RelationKeys::new(local_key, foreign_key))
    }

    pub const fn one(self) -> HasOne<P, R> {
        HasOne::from_many(self)
    }

    pub(super) const fn morph(
        local_key: &'static str,
        foreign_key: &'static str,
        type_column: &'static str,
        alias: &'static str,
    ) -> Self {
        Self(RelationKeys::new(local_key, foreign_key).with_discriminator(type_column, alias))
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

    pub async fn save(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        let key = parent
            .getAttribute(self.0.parent)
            .filter(|value| !matches!(value, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship parent key"))?;
        let connection = self.0.connection(parent)?;
        if let Some(current) = &model.state().connection
            && !current.same_query_source(&connection)
        {
            return Err(DatabaseError::InvalidQuery(
                "mixed relationship connections",
            ));
        }
        model.state_mut().connection = Some(connection);
        model.setAttribute(self.0.related, key.clone());
        if let Some((column, alias)) = self.0.discriminator {
            model.setAttribute(column, alias);
        }
        model.save().await
    }

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let mut model = R::newInstance(attributes)?;
        self.save(parent, &mut model).await?;
        Ok(model)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> Relation for HasMany<P, R> {
    type Parent = P;
    type Output = Vec<Arc<R>>;

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(self.0.match_many(parents))
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, R: Model> ExistenceRelation for HasMany<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.existence_query(connection, parent_table)
    }
}
