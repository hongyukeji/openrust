use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use crate::{http::HttpError, routing::BindingContext};

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

    /// Uses the same keys and constraints with a composed parent model.
    #[must_use]
    #[allow(non_snake_case, reason = "Matches the Eloquent relationship API")]
    pub fn forParent<M: Model>(self) -> HasMany<M, R> {
        HasMany(self.0.for_parent())
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

    /// Resolves one routed child through this parent's relationship.
    #[allow(non_snake_case, reason = "Matches Laravel's route binding API")]
    pub async fn resolveRouteBinding(
        &self,
        parent: &P,
        binding: BindingContext<'_>,
    ) -> Result<Option<R>, HttpError> {
        use super::super::route_binding::{http_error, lookup};

        let Some(key) = parent
            .getAttribute(self.0.parent)
            .filter(|key| !matches!(key, Value::Null))
        else {
            return Ok(None);
        };
        let connection = self.0.connection(parent).map_err(http_error)?;
        let query = self
            .0
            .configure(R::queryOn(&connection))
            .map_err(http_error)?;
        let column = query.qualifyColumn(self.0.related);
        lookup(query.require_relation_value(&column, key.clone()), binding)
            .await
            .map_err(http_error)
    }

    /// Creates a child with an initial counter or increments a matching child.
    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn incrementOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        column: &str,
        default: impl Into<Value>,
        step: impl Into<Value>,
        mut extra: Attributes,
    ) -> DatabaseResult<R> {
        use super::super::persistence::{increment_related, is_unique_violation, validate_counter};

        let default = default.into();
        let step = step.into();
        validate_counter::<R>(column, &default, &step, &extra)?;
        let key = parent
            .getAttribute(self.0.parent)
            .filter(|key| !matches!(key, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship parent key"))?;
        let connection = self.0.connection(parent)?;
        let lookup = || {
            let query = self.0.configure(R::queryOn(&connection))?;
            let foreign = query.qualifyColumn(self.0.related);
            Ok::<_, DatabaseError>(
                self.0
                    .select_one(query)?
                    .require_relation_value(&foreign, key.clone())
                    .restrict_relation_attributes(&attributes),
            )
        };

        let model = match lookup()?.first().await? {
            Some(model) => model,
            None => {
                let mut values = attributes.clone();
                values.extend(extra.clone());
                values.insert(column.into(), default);
                let mut model = self
                    .0
                    .configure(R::queryOn(&connection))?
                    .makeAsync(values)
                    .await?;
                match self.save(parent, &mut model).await {
                    Ok(true) => return Ok(model),
                    Ok(false) => {
                        return Err(DatabaseError::InvalidQuery(
                            "related model creation cancelled",
                        ));
                    }
                    Err(error) if is_unique_violation(&error) => {
                        lookup()?.useWritePdo().first().await?.ok_or(error)?
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        extra.remove(column);
        increment_related(model, column, step, extra).await
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
        let mut model = if attributes.keys().any(|name| {
            super::super::casts::needs_async::<R>(
                name,
                &super::super::casts::CastOverrides::default(),
            )
        }) {
            let mut model = R::newInstanceOn(&self.0.connection(parent)?, Attributes::new())?;
            Box::pin(model.fillAsync(attributes)).await?;
            model
        } else {
            R::newInstance(attributes)?
        };
        self.save(parent, &mut model).await?;
        Ok(model)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> Relation for HasMany<P, R> {
    type Parent = P;
    type Output = Vec<Arc<R>>;

    fn attachAutoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn autoloadOutput(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::many(output)
    }

    fn queueableRelations(output: &Self::Output) -> Vec<String> {
        super::super::queueable::common_relations(output.iter().map(Arc::as_ref))
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

impl<P: Model, R: Model> HasMany<P, R> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        Box::pin(self.0.match_many(parents, transaction))
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

impl<P: Model, R: Model> super::NestedRelation for HasMany<P, R> {
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

impl<P: Model, R: Model> super::ConstrainableRelation for HasMany<P, R> {
    type Related = R;

    fn constrain_query(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.0.append_constraint(configure);
        self
    }
}
