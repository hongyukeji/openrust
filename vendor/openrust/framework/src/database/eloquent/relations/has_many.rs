use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    query,
};

use crate::{http::HttpError, routing::BindingContext};

use crate::pagination::{
    CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, Paginator,
};

use super::RelatedValues;

use super::{ExistenceRelation, HasOne, Relation, relation::RelationKeys};

/// Links a parent key to a foreign key on many related models.
pub struct HasMany<P: Model, R: Model>(pub(super) RelationKeys<P, R>);

#[allow(
    non_snake_case,
    reason = "Matches Laravel's relationship pagination API"
)]
impl<P: Model, R: Model> HasMany<P, R> {
    pub async fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.pagination_query(parent)?.paginate(per_page).await
    }

    pub async fn paginateWith(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.pagination_query(parent)?.paginateWith(options).await
    }

    pub async fn simplePaginate(&self, parent: &P, per_page: u64) -> DatabaseResult<Paginator<R>> {
        self.pagination_query(parent)?
            .simplePaginate(per_page)
            .await
    }

    pub async fn simplePaginateWith(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<Paginator<R>> {
        self.pagination_query(parent)?
            .simplePaginateWith(options)
            .await
    }

    pub async fn cursorPaginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.pagination_query(parent)?
            .cursorPaginate(per_page)
            .await
    }

    pub async fn cursorPaginateWith(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.pagination_query(parent)?
            .cursorPaginateWith(options)
            .await
    }

    fn pagination_query(&self, parent: &P) -> DatabaseResult<Builder<'static, R>> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::queryOn(&connection))?;
        let foreign = query.qualifyColumn(self.0.related);
        let parents = super::relation::ParentKeys::new(&[parent], self.0.parent)?;
        Ok(query.restrict_relation(&foreign, &parents.bindings))
    }
}

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

    /// Hydrates the named inverse relation from the parent already in memory.
    #[must_use]
    pub fn chaperone(mut self, relation: &'static str) -> Self {
        self.0.inverse = Some(relation);
        self
    }

    #[must_use]
    pub fn inverse(self, relation: &'static str) -> Self {
        self.chaperone(relation)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's inverse relationship API")]
    #[must_use]
    pub fn withoutChaperone(mut self) -> Self {
        self.0.inverse = None;
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's inverse relationship API")]
    #[must_use]
    pub fn withoutInverse(self) -> Self {
        self.withoutChaperone()
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

    /// Finds the related primary key or prepares an unsaved child for this parent.
    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn findOrNew(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        match self.find(parent, key).await? {
            Some(model) => Ok(model),
            None => self.make(parent, Attributes::new()).await,
        }
    }

    super::lookup::relation_lookup!();

    pub(super) async fn lookup_models(
        &self,
        parent: &P,
        keys: &Attributes,
        limit: u64,
    ) -> DatabaseResult<Vec<R>> {
        let collection = self.lookup(parent, keys, false)?.limit(limit).get().await?;
        Ok(collection.into_parts().0)
    }

    pub(super) async fn lookup_many(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        if parent
            .getAttribute(self.0.parent)
            .is_none_or(|key| matches!(key, Value::Null))
        {
            return Ok(R::newCollection(Vec::new()));
        }
        super::lookup::LookupSelection::Keys(keys)
            .apply(self.lookup(parent, &Attributes::new(), false)?)
            .get()
            .await
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
        self.0.hydrate_inverse(parent, model);
        model.save().await
    }

    /// Constructs an unsaved child with the relationship attributes.
    pub async fn make(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::queryOn(&connection))?;
        let mut model = query.make_with_attributes_async(attributes).await?;
        self.apply_creation_attributes(parent, &mut model, query.pending_attributes)
            .await?;
        self.0.hydrate_inverse(parent, &mut model);
        Ok(model)
    }

    /// Constructs unsaved children in input order.
    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub async fn makeMany(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        let mut models = Vec::new();
        for attributes in records {
            models.push(self.make(parent, attributes).await?);
        }
        Ok(R::newCollection(models))
    }

    pub(in crate::database::eloquent) fn factory_attributes(
        &self,
        parent: &P,
    ) -> DatabaseResult<(Attributes, Attributes)> {
        let connection = self.0.connection(parent)?;
        let defaults = self
            .0
            .configure(R::queryOn(&connection))?
            .pending_attributes;
        let mut model = R::from_state(Default::default());
        self.set_parent_attributes(parent, &mut model)?;
        Ok((defaults, model.state().attributes.clone()))
    }

    pub(in crate::database::eloquent) async fn save_factory_child(
        &self,
        parent: &P,
        model: &mut R,
    ) -> DatabaseResult<bool> {
        let connection = self.0.connection(parent)?;
        if model
            .state()
            .connection
            .as_ref()
            .is_some_and(|current| !current.same_query_source(&connection))
        {
            return Err(DatabaseError::InvalidQuery(
                "mixed relationship connections",
            ));
        }
        self.0.hydrate_inverse(parent, model);
        model.save().await
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
        self.0.hydrate_inverse(parent, &mut model);
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
    type Output = super::super::RelatedCollection<R>;

    fn isEmptyCollection(output: &Self::Output) -> bool {
        output.is_empty()
    }

    fn attachAutoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn pushLoaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(async move {
            let (models, result) = context.push_many(output).await;
            (models.map(|models| output.with_models(models)), result)
        })
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
        Box::pin(async move {
            let results = self.0.match_many(parents, transaction).await?;
            Ok(results
                .into_iter()
                .map(super::super::RelatedCollection::from_models)
                .collect())
        })
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

    fn with_path_columns(mut self, columns: Vec<String>) -> DatabaseResult<Self> {
        self.0
            .append_constraint(move |query| query.select_path_columns(&columns));
        Ok(self)
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
