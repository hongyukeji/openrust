use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, Model, ModelState,
        concerns::{
            has_events::{ModelEvent, fire_with_transaction},
            has_timestamps::timestamp,
        },
        touches::TouchedOwners,
    },
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

    pub(in crate::database::eloquent) async fn touch_owner(
        &self,
        model: &P,
        attributes: &Attributes,
        visited: &mut TouchedOwners,
        mut transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        let Some(foreign_key) = attributes
            .get(self.0.parent)
            .filter(|key| **key != Value::Null)
        else {
            return Ok(());
        };
        let connection = self.0.connection(model)?;
        let relation = self
            .0
            .configure(R::queryOn(&connection))?
            .restrict_relation(self.0.related, std::slice::from_ref(foreign_key));
        let casts = relation.casts.clone();
        let query = relation.scoped()?.0.useWritePdo();
        let row = match transaction.as_deref_mut() {
            Some(tx) => query.in_transaction(tx)?.first().await?,
            None => query.first().await?,
        };
        let Some(row) = row else {
            return Ok(());
        };
        let mut attributes: Attributes = row.into_columns().into_iter().collect();
        let metadata = R::metadata();
        let key = attributes
            .get(metadata.primary_key)
            .filter(|key| **key != Value::Null)
            .cloned()
            .ok_or(DatabaseError::InvalidQuery("touch owner primary key"))?;
        if !visited.visit::<R>(&connection, Some(&key))? {
            return Ok(());
        }
        if let Some(column) = metadata.updated_at {
            let time = timestamp(&connection, metadata.date_format)?;
            let query = self
                .0
                .configure(R::queryOn(&connection))?
                .restrict_relation(self.0.related, std::slice::from_ref(foreign_key))
                .restrict_relation(metadata.primary_key, &[key])
                .scoped()?
                .0;
            let update = Attributes::from([(column.into(), time.clone())]);
            match transaction.as_deref_mut() {
                Some(tx) => query.in_transaction(tx)?.update(update).await?,
                None => query.update(update).await?,
            };
            attributes.insert(column.into(), time);
        }
        let mut owner = R::from_state(ModelState::hydrated_attributes(
            attributes.clone(),
            connection.clone(),
            casts,
        ));
        fire_with_transaction(
            &mut owner,
            &connection,
            ModelEvent::Saved,
            false,
            transaction.as_deref_mut(),
        )
        .await?;
        R::touches()
            .run(&owner, &attributes, visited, transaction)
            .await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> Relation for BelongsTo<P, R> {
    type Parent = P;
    type Output = Option<Arc<R>>;

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::one(output)
    }

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
