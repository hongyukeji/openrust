use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use crate::{http::HttpError, routing::BindingContext};

use super::RelatedValues;

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
    pub async fn firstOrNew<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        if let Some(model) = self.lookup(parent, &attributes, false)?.first().await? {
            return Ok(model);
        }
        let mut attributes = attributes;
        attributes.extend(values.into().resolve());
        let model = self.make(parent, attributes).await?;
        Ok(model)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn firstOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.find_or_create(parent, attributes, &mut values.into(), false, false)
            .await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn createOrFirst<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.find_or_create(parent, attributes, &mut values.into(), false, true)
            .await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn updateOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        let mut values = values.into();
        let mut model = self
            .find_or_create(parent, attributes, &mut values, false, false)
            .await?;
        if !model.state().wasRecentlyCreated() {
            model.fillAsync(values.resolve()).await?;
            model.save().await?;
        }
        Ok(model)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn restoreOrCreate<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        super::require_soft_deletes::<R>()?;
        let mut model = self
            .find_or_create(parent, attributes, &mut values.into(), true, false)
            .await?;
        model.restore().await?;
        Ok(model)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn createOrRestore<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        super::require_soft_deletes::<R>()?;
        let mut model = self
            .find_or_create(parent, attributes, &mut values.into(), true, true)
            .await?;
        model.restore().await?;
        Ok(model)
    }

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
        use super::super::persistence::{increment_related, validate_counter};

        let default = default.into();
        let step = step.into();
        validate_counter::<R>(column, &default, &step, &extra)?;
        extra.remove(column);
        let mut values = extra.clone();
        values.insert(column.into(), default);
        let model = self
            .find_or_create(parent, attributes, &mut values.into(), false, false)
            .await?;
        if model.state().wasRecentlyCreated() {
            return Ok(model);
        }
        increment_related(model, column, step, extra).await
    }

    async fn find_or_create(
        &self,
        parent: &P,
        attributes: Attributes,
        values: &mut RelatedValues<'_>,
        include_trashed: bool,
        insert_first: bool,
    ) -> DatabaseResult<R> {
        if !insert_first
            && let Some(model) = self
                .lookup(parent, &attributes, include_trashed)?
                .first()
                .await?
        {
            return Ok(model);
        }
        let mut insert = attributes.clone();
        insert.extend(values.resolve());
        let mut model = self.make(parent, insert).await?;
        match model.save().await {
            Ok(true) => Ok(model),
            Ok(false) => Err(DatabaseError::InvalidQuery(
                "related model creation cancelled",
            )),
            Err(error) if super::super::persistence::is_unique_violation(&error) => self
                .lookup(parent, &attributes, include_trashed)?
                .useWritePdo()
                .first()
                .await?
                .ok_or(error),
            Err(error) => Err(error),
        }
    }

    fn lookup(
        &self,
        parent: &P,
        attributes: &Attributes,
        include_trashed: bool,
    ) -> DatabaseResult<Builder<'static, R>> {
        let key = parent
            .getAttribute(self.0.parent)
            .filter(|key| !matches!(key, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship parent key"))?;
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::queryOn(&connection))?;
        let query = if include_trashed {
            query.withTrashed()
        } else {
            query
        };
        let foreign = query.qualifyColumn(self.0.related);
        Ok(self
            .0
            .select_one(query)?
            .require_relation_value(&foreign, key.clone())
            .restrict_relation_attributes(attributes))
    }

    /// Writes related rows, assigning the parent key and any morph discriminator.
    pub async fn upsert(
        &self,
        parent: &P,
        mut rows: Vec<Attributes>,
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let key = parent
            .getAttribute(self.0.parent)
            .filter(|value| !matches!(value, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship parent key"))?;
        for row in &mut rows {
            if let Some((column, alias)) = self.0.discriminator {
                row.insert(column.into(), alias.into());
            }
            row.insert(self.0.related.into(), key.clone());
        }
        let connection = self.0.connection(parent)?;
        self.0
            .configure(R::queryOn(&connection))?
            .upsert(rows, unique_by, update)
            .await
    }

    pub async fn save(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::queryOn(&connection))?;
        self.apply_creation_attributes(parent, model, query.pending_attributes)
            .await?;
        model.save().await
    }

    async fn make(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::queryOn(&connection))?;
        let mut model = query.make_with_attributes_async(attributes).await?;
        self.apply_creation_attributes(parent, &mut model, query.pending_attributes)
            .await?;
        Ok(model)
    }

    async fn apply_creation_attributes(
        &self,
        parent: &P,
        model: &mut R,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        self.set_parent_attributes(parent, model)?;
        for (column, value) in attributes {
            if model.getAttribute(&column).is_none() {
                model.setAttributeValueAsync(&column, value).await?;
            }
        }
        Ok(())
    }

    fn set_parent_attributes(&self, parent: &P, model: &mut R) -> DatabaseResult<()> {
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
        Ok(())
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn saveQuietly(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        R::withoutEvents(self.save(parent, model)).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn saveMany(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        for model in models {
            self.save(parent, model).await?;
        }
        Ok(())
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn saveManyQuietly(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        R::withoutEvents(self.saveMany(parent, models)).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn createQuietly(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        R::withoutEvents(self.create(parent, attributes)).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn createMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        let mut models = Vec::new();
        for attributes in records {
            models.push(self.create(parent, attributes).await?);
        }
        Ok(models)
    }

    #[allow(non_snake_case, reason = "Matches Laravel’s relationship write API")]
    pub async fn createManyQuietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        R::withoutEvents(self.createMany(parent, records)).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
    pub async fn forceCreate(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::queryOn(&connection))?;
        let mut model = R::newInstanceOn(&connection, Attributes::new())?;
        for (column, value) in query.creation_attributes(attributes) {
            model.setAttributeValueAsync(&column, value).await?;
        }
        self.set_parent_attributes(parent, &mut model)?;
        model.save().await?;
        Ok(model)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
    pub async fn forceCreateQuietly(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<R> {
        R::withoutEvents(self.forceCreate(parent, attributes)).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
    pub async fn forceCreateMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        let mut models = Vec::new();
        for attributes in records {
            models.push(self.forceCreate(parent, attributes).await?);
        }
        Ok(models)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's relationship write API")]
    pub async fn forceCreateManyQuietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        R::withoutEvents(self.forceCreateMany(parent, records)).await
    }

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let mut model = self.make(parent, attributes).await?;
        model.save().await?;
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
