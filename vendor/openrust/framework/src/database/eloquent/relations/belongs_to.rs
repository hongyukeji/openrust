use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model, touches::TouchedOwners},
    query,
};

use super::super::concerns::has_relationships::update_association_cache;

use super::{
    ExistenceRelation, Relation,
    default_model::{ConfigureDefault, match_defaults},
    relation::RelationKeys,
};

/// Resolves a model's foreign key against a related owner's key.
pub struct BelongsTo<P: Model, R: Model>(
    RelationKeys<P, R>,
    Option<Box<ConfigureDefault<P, R>>>,
    Option<&'static str>,
);

#[allow(non_snake_case, reason = "Matches Laravel's default relationship API")]
impl<P: Model, R: Model> BelongsTo<P, R> {
    pub(in crate::database::eloquent) fn identity_columns(&self) -> (&str, &str) {
        (self.0.parent, self.0.related)
    }

    pub const fn new(foreign_key: &'static str, owner_key: &'static str) -> Self {
        Self(RelationKeys::new(foreign_key, owner_key), None, None)
    }

    /// Names the relationship cache entry updated by association.
    #[must_use]
    pub const fn named(mut self, name: &'static str) -> Self {
        self.2 = Some(name);
        self
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
        update_association_cache::<Self>(model, self.2, || {
            Some(Some(Arc::new(R::from_state(related.state().clone()))))
        });
        model.setAttribute(self.0.parent, value.clone());
        Ok(())
    }

    /// Sets the foreign key and invalidates the cached owner.
    pub fn associateKey(&self, model: &mut P, key: impl Into<Value>) {
        update_association_cache::<Self>(model, self.2, || None);
        model.setAttribute(self.0.parent, key);
    }

    pub fn dissociate(&self, model: &mut P) {
        update_association_cache::<Self>(model, self.2, || Some(None));
        model.setAttribute(self.0.parent, Value::Null);
    }

    /// Alias for `dissociate`.
    pub fn disassociate(&self, model: &mut P) {
        self.dissociate(model);
    }

    pub(in crate::database::eloquent) async fn touch_owner(
        &self,
        model: &P,
        attributes: &Attributes,
        visited: &mut TouchedOwners,
        transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        self.0
            .touch_owner(model, attributes, visited, transaction)
            .await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> Relation for BelongsTo<P, R> {
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

impl<P: Model, R: Model> BelongsTo<P, R> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        Box::pin(async move {
            let matches = self.0.match_many(parents, transaction).await?;
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

impl<P: Model, R: Model> super::NestedRelation for BelongsTo<P, R> {
    type Related = R;

    fn with_nested(mut self, nested: super::super::EagerLoads<R>) -> Self {
        self.0.with_nested(nested);
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

impl<P: Model, R: Model> super::ConstrainableRelation for BelongsTo<P, R> {
    type Related = R;

    fn constrain_query(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.0.append_constraint(configure);
        self
    }
}
