use futures_util::TryStreamExt as _;
use std::sync::Arc;

use crate::database::{
    Connection, DatabaseCursor, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model},
    events::with_query_caller,
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

impl<P: Model, R: Model> HasMany<P, R> {
    #[track_caller]
    pub fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.pagination_query(parent)?.paginate(per_page).await
        })
    }

    #[track_caller]
    pub fn paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.pagination_query(parent)?.paginate_with(options).await
        })
    }

    #[track_caller]
    pub fn simple_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<Paginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.pagination_query(parent)?
                .simple_paginate(per_page)
                .await
        })
    }

    #[track_caller]
    pub fn simple_paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<Paginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.pagination_query(parent)?
                .simple_paginate_with(options)
                .await
        })
    }

    #[track_caller]
    pub fn cursor_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.pagination_query(parent)?
                .cursor_paginate(per_page)
                .await
        })
    }

    #[track_caller]
    pub fn cursor_paginate_with(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.pagination_query(parent)?
                .cursor_paginate_with(options)
                .await
        })
    }

    /// Reads this parent's related models in ordered batches.
    #[track_caller]
    pub fn chunk<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
    ) -> impl Future<Output = DatabaseResult<bool>>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        with_query_caller(std::panic::Location::caller(), async move {
            self.0
                .select_one(self.pagination_query(parent)?)?
                .chunk(count, callback)
                .await
        })
    }

    super::key_chunks::lazy_chunks!(R);

    pub(super) fn lazy_relation<'a>(
        &'a self,
        parent: &'a P,
        chunk_size: u64,
        key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
    ) -> DatabaseCursor<'a, R> {
        Box::pin(async_stream::try_stream! {
            let query = self.0.select_one(self.pagination_query(parent)?)?;
            let mut models = if let Some((column, alias, descending)) = key {
                query.lazy_by_key(chunk_size, column, alias, descending)
            } else {
                query.lazy(chunk_size)
            };
            while let Some(model) = models.try_next().await? {
                yield model;
            }
        })
    }

    #[track_caller]
    pub fn each<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(R, crate::support::ArrayKey) -> Fut,
        count: u64,
    ) -> impl Future<Output = DatabaseResult<bool>>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        with_query_caller(std::panic::Location::caller(), async move {
            self.0
                .select_one(self.pagination_query(parent)?)?
                .each(callback, count)
                .await
        })
    }

    #[track_caller]
    pub fn each_by_id<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(R, i64) -> Fut,
        count: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> impl Future<Output = DatabaseResult<bool>>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        with_query_caller(std::panic::Location::caller(), async move {
            self.0
                .select_one(self.pagination_query(parent)?)?
                .each_by_id(callback, count, column, alias)
                .await
        })
    }

    super::key_chunks::key_chunks!(crate::database::eloquent::Collection<R>);

    #[track_caller]
    pub fn ordered_chunk_by_id<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> impl Future<Output = DatabaseResult<bool>>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        with_query_caller(std::panic::Location::caller(), async move {
            self.0
                .select_one(self.pagination_query(parent)?)?
                .ordered_chunk_by_id(count, callback, column, alias, descending)
                .await
        })
    }

    /// Streams models constrained to this parent.
    #[track_caller]
    pub fn cursor<'a>(&'a self, parent: &'a P) -> DatabaseCursor<'a, R> {
        Box::pin(crate::database::events::stream_with_query_caller(
            std::panic::Location::caller(),
            async_stream::try_stream! {
                let query = self.pagination_query(parent)?;
                let mut models = self.0.select_one(query)?.cursor();
                while let Some(model) = models.try_next().await? {
                    yield model;
                }
            },
        ))
    }

    pub(super) fn single_pagination_query(
        &self,
        parent: &P,
    ) -> DatabaseResult<Builder<'static, R>> {
        self.0.select_one(self.pagination_query(parent)?)
    }

    fn pagination_query(&self, parent: &P) -> DatabaseResult<Builder<'static, R>> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::query_on(&connection))?;
        let foreign = query.qualify_column(self.0.related);
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
    pub fn for_parent<M: Model>(self) -> HasMany<M, R> {
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

    #[must_use]
    pub fn without_chaperone(mut self) -> Self {
        self.0.inverse = None;
        self
    }

    #[must_use]
    pub fn without_inverse(self) -> Self {
        self.without_chaperone()
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
    pub async fn resolve_route_binding(
        &self,
        parent: &P,
        binding: BindingContext<'_>,
    ) -> Result<Option<R>, HttpError> {
        use super::super::route_binding::lookup;

        let Some(key) = parent
            .get_attribute(self.0.parent)
            .filter(|key| !matches!(key, Value::Null))
        else {
            return Ok(None);
        };
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::query_on(&connection))?;
        let column = query.qualify_column(self.0.related);
        lookup(query.require_relation_value(&column, key.clone()), binding)
            .await
            .map_err(Into::into)
    }

    query_callers! {
    /// Finds the related primary key or prepares an unsaved child for this parent.
    pub async fn find_or_new(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        match self.find(parent, key).await? {
            Some(model) => Ok(model),
            None => self.make(parent, Attributes::new()).await,
        }
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
            .get_attribute(self.0.parent)
            .is_none_or(|key| matches!(key, Value::Null))
        {
            return Ok(R::new_collection(Vec::new()));
        }
        super::lookup::LookupSelection::Keys(keys)
            .apply(self.lookup(parent, &Attributes::new(), false)?)
            .get()
            .await
    }

    query_callers! {
    /// Creates a child with an initial counter or increments a matching child.
    pub async fn first_or_new<'a>(
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

    pub async fn first_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.find_or_create(parent, attributes, &mut values.into(), false, false)
            .await
    }

    pub async fn create_or_first<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        self.find_or_create(parent, attributes, &mut values.into(), false, true)
            .await
    }

    pub async fn update_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
    ) -> DatabaseResult<R> {
        let mut values = values.into();
        let mut model = self
            .find_or_create(parent, attributes, &mut values, false, false)
            .await?;
        if !model.state().was_recently_created() {
            model.fill_async(values.resolve()).await?;
            model.save().await?;
        }
        Ok(model)
    }

    pub async fn restore_or_create<'a>(
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

    pub async fn create_or_restore<'a>(
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

    pub async fn increment_or_create(
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
        if model.state().was_recently_created() {
            return Ok(model);
        }
        increment_related(model, column, step, extra).await
    }
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
                .use_write_pdo()
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
            .get_attribute(self.0.parent)
            .filter(|key| !matches!(key, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship parent key"))?;
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::query_on(&connection))?;
        let query = if include_trashed {
            query.with_trashed()
        } else {
            query
        };
        let foreign = query.qualify_column(self.0.related);
        Ok(self
            .0
            .select_one(query)?
            .require_relation_value(&foreign, key.clone())
            .restrict_relation_attributes(attributes))
    }

    query_callers! {
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
            .get_attribute(self.0.parent)
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
            .configure(R::query_on(&connection))?
            .upsert(rows, unique_by, update)
            .await
    }

    pub async fn save(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::query_on(&connection))?;
        self.apply_creation_attributes(parent, model, query.pending_attributes)
            .await?;
        self.0.hydrate_inverse(parent, model);
        model.save().await
    }
    }

    /// Constructs an unsaved child with the relationship attributes.
    pub async fn make(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::query_on(&connection))?;
        let mut model = query.make_with_attributes_async(attributes).await?;
        self.apply_creation_attributes(parent, &mut model, query.pending_attributes)
            .await?;
        self.0.hydrate_inverse(parent, &mut model);
        Ok(model)
    }

    /// Constructs unsaved children in input order.
    pub async fn make_many(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        let mut models = Vec::new();
        for attributes in records {
            models.push(self.make(parent, attributes).await?);
        }
        Ok(R::new_collection(models))
    }

    pub(in crate::database::eloquent) fn factory_attributes(
        &self,
        parent: &P,
    ) -> DatabaseResult<(Attributes, Attributes)> {
        let connection = self.0.connection(parent)?;
        let defaults = self
            .0
            .configure(R::query_on(&connection))?
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
            if model.get_attribute(&column).is_none() {
                model.set_attribute_value_async(&column, value).await?;
            }
        }
        Ok(())
    }

    fn set_parent_attributes(&self, parent: &P, model: &mut R) -> DatabaseResult<()> {
        let key = parent
            .get_attribute(self.0.parent)
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
        model.set_attribute(self.0.related, key.clone());
        if let Some((column, alias)) = self.0.discriminator {
            model.set_attribute(column, alias);
        }
        Ok(())
    }

    query_callers! {
    pub async fn save_quietly(&self, parent: &P, model: &mut R) -> DatabaseResult<bool> {
        R::without_events(self.save(parent, model)).await
    }

    pub async fn save_many(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        for model in models {
            self.save(parent, model).await?;
        }
        Ok(())
    }

    pub async fn save_many_quietly(&self, parent: &P, models: &mut [R]) -> DatabaseResult<()> {
        R::without_events(self.save_many(parent, models)).await
    }

    pub async fn create_quietly(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        R::without_events(self.create(parent, attributes)).await
    }

    pub async fn create_many(
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

    pub async fn create_many_quietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        R::without_events(self.create_many(parent, records)).await
    }

    pub async fn force_create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let connection = self.0.connection(parent)?;
        let query = self.0.configure(R::query_on(&connection))?;
        let mut model = R::new_instance_on(&connection, Attributes::new())?;
        for (column, value) in query.creation_attributes(attributes) {
            model.set_attribute_value_async(&column, value).await?;
        }
        self.set_parent_attributes(parent, &mut model)?;
        self.0.hydrate_inverse(parent, &mut model);
        model.save().await?;
        Ok(model)
    }

    pub async fn force_create_quietly(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<R> {
        R::without_events(self.force_create(parent, attributes)).await
    }

    pub async fn force_create_many(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        let mut models = Vec::new();
        for attributes in records {
            models.push(self.force_create(parent, attributes).await?);
        }
        Ok(models)
    }

    pub async fn force_create_many_quietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
    ) -> DatabaseResult<Vec<R>> {
        R::without_events(self.force_create_many(parent, records)).await
    }

    pub async fn create(&self, parent: &P, attributes: Attributes) -> DatabaseResult<R> {
        let mut model = self.make(parent, attributes).await?;
        model.save().await?;
        Ok(model)
    }
    }
}

impl<P: Model, R: Model> Relation for HasMany<P, R> {
    type Parent = P;
    type Output = super::super::RelatedCollection<R>;

    fn is_empty_collection(output: &Self::Output) -> bool {
        output.is_empty()
    }

    fn attach_autoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn push_loaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(async move {
            let (models, result) = context.push_many(output).await;
            (models.map(|models| output.with_models(models)), result)
        })
    }

    fn autoload_output(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::many(output)
    }

    fn queueable_relations(output: &Self::Output) -> Vec<String> {
        super::super::queueable::common_relations(output.iter().map(Arc::as_ref))
    }

    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, None),
        ))
    }

    fn match_results_in_transaction<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, Some(transaction)),
        ))
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

impl<P: Model, R: Model> ExistenceRelation for HasMany<P, R> {
    fn get_relation_existence_query(
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
