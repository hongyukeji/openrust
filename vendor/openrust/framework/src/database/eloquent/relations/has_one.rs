use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use super::{
    ExistenceRelation, HasMany, OneOfManyAggregate, Relation,
    default_model::{ConfigureDefault, match_defaults},
    one_of_many::OneOfMany,
};

/// Links a parent key to the first related model with a matching foreign key.
pub struct HasOne<P: Model, R: Model>(
    pub(super) HasMany<P, R>,
    Option<Box<ConfigureDefault<P, R>>>,
);

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> HasOne<P, R> {
    pub const fn new(local_key: &'static str, foreign_key: &'static str) -> Self {
        HasMany::new(local_key, foreign_key).one()
    }

    pub(super) const fn from_many(relation: HasMany<P, R>) -> Self {
        Self(relation, None)
    }

    /// Configures an unsaved related model when there is no matching row.
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
        self.0 = self.0.constrain(configure);
        self
    }

    pub async fn is(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        match related {
            Some(related) => self.0.0.matches_selected(parent, related).await,
            None => Ok(false),
        }
    }

    pub async fn isNot(&self, parent: &P, related: Option<&R>) -> DatabaseResult<bool> {
        Ok(!self.is(parent, related).await?)
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
        self.0.0.selection = Some(OneOfMany::new::<R>(
            columns.iter().copied(),
            OneOfManyAggregate::Max,
        ));
        self
    }

    #[must_use]
    pub fn ofManyBy(mut self, column: &'static str, aggregate: OneOfManyAggregate) -> Self {
        self.0.0.selection = Some(OneOfMany::new::<R>([(column, aggregate)], aggregate));
        self
    }

    pub fn whereCandidates(
        mut self,
        filter: impl Fn(query::Conditions) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        self.0
            .0
            .selection
            .as_mut()
            .ok_or(DatabaseError::InvalidQuery(
                "configure one-of-many before its candidates",
            ))?
            .filter(filter);
        Ok(self)
    }

    pub async fn save(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        self.0.save(parent, model).await
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

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        self.0.create(parent, attributes).await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> Relation for HasOne<P, R> {
    type Parent = P;
    type Output = Option<Arc<R>>;

    fn attachAutoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
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

impl<P: Model, R: Model> HasOne<P, R> {
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
            let keys = &self.0.0;
            match_defaults(
                parents,
                matches.into_iter().map(|rows| rows.into_iter().next()),
                self.1.as_deref(),
                keys,
                |parent| {
                    let key = parent
                        .getAttribute(keys.parent)
                        .cloned()
                        .unwrap_or(Value::Null);
                    let mut attributes = Attributes::from([(keys.related.into(), key)]);
                    if let Some((column, alias)) = keys.discriminator {
                        attributes.insert(column.into(), alias.into());
                    }
                    attributes
                },
            )
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, R: Model> ExistenceRelation for HasOne<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.0.0.existence_query(connection, parent_table)
    }
}

impl<P: Model, R: Model> super::NestedRelation for HasOne<P, R> {
    type Related = R;

    fn with_nested(mut self, nested: super::super::EagerLoads<R>) -> Self {
        self.0.0.with_nested(nested);
        self
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

impl<P: Model, R: Model> super::ConstrainableRelation for HasOne<P, R> {
    type Related = R;

    fn constrain_query(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.0.0.append_constraint(configure);
        self
    }
}
