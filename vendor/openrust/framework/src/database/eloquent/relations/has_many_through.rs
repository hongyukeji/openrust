use futures_util::TryStreamExt as _;
use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use crate::database::{
    Connection, DatabaseCursor, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, Model, ModelState,
        concerns::{
            has_relationships::load_eager_with_transaction, queries_relationships::cast_existence,
        },
    },
    query,
};

use crate::pagination::{
    CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, Paginator,
};

use super::{
    ExistenceRelation, HasOneThrough, Relation,
    one_of_many::OneOfMany,
    relation::{ParentKeys, RelationKeys, dictionary_key},
};

struct ThroughLookup<'a> {
    selection: super::lookup::LookupSelection<'a>,
    include_trashed: bool,
}

fn owned_model<R: Model>(model: Arc<R>) -> R {
    Arc::try_unwrap(model).unwrap_or_else(|model| R::from_state(model.state().clone()))
}

/// Links a parent to related models through an intermediate model.
pub struct HasManyThrough<P: Model, I: Model, R: Model> {
    pub(super) keys: RelationKeys<P, R>,
    first_key: &'static str,
    second_local_key: &'static str,
    include_trashed_parents: bool,
    intermediate_morph: Option<(&'static str, &'static str)>,
    pub(super) selection: Option<OneOfMany>,
    intermediate: PhantomData<fn() -> I>,
}

#[allow(non_snake_case, reason = "Matches Laravel's through relation API")]
impl<P: Model, I: Model, R: Model> HasManyThrough<P, I, R> {
    pub const fn new(
        first_key: &'static str,
        second_key: &'static str,
        local_key: &'static str,
        second_local_key: &'static str,
    ) -> Self {
        Self {
            keys: RelationKeys::new(local_key, second_key),
            first_key,
            second_local_key,
            include_trashed_parents: false,
            intermediate_morph: None,
            selection: None,
            intermediate: PhantomData,
        }
    }

    pub(super) fn with_intermediate_morph(
        mut self,
        morph: Option<(&'static str, &'static str)>,
    ) -> Self {
        self.intermediate_morph = morph;
        self
    }

    #[must_use]
    pub fn one(self) -> HasOneThrough<P, I, R> {
        HasOneThrough::from_many(self)
    }

    #[must_use]
    pub fn withTrashedParents(mut self) -> Self {
        self.include_trashed_parents = true;
        self
    }

    #[must_use]
    pub fn constrain(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.keys.constrain(configure);
        self
    }

    pub async fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.paginateWith(
            parent,
            PageOptions {
                perPage: Some(per_page),
                ..PageOptions::default()
            },
        )
        .await
    }

    pub async fn paginateWith(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<R>> {
        self.pagination_query(parent, options.columns)?
            .paginateWith(options)
            .await
    }

    pub async fn simplePaginate(&self, parent: &P, per_page: u64) -> DatabaseResult<Paginator<R>> {
        self.simplePaginateWith(
            parent,
            PageOptions {
                perPage: Some(per_page),
                ..PageOptions::default()
            },
        )
        .await
    }

    pub async fn simplePaginateWith(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> DatabaseResult<Paginator<R>> {
        self.pagination_query(parent, options.columns)?
            .simplePaginateWith(options)
            .await
    }

    pub async fn cursorPaginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.cursorPaginateWith(
            parent,
            CursorOptions {
                perPage: Some(per_page),
                ..CursorOptions::default()
            },
        )
        .await
    }

    pub async fn cursorPaginateWith(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> DatabaseResult<CursorPaginator<R>> {
        self.pagination_query(parent, options.columns)?
            .cursorPaginateWith(options)
            .await
    }

    /// Reads this parent's related models in ordered batches.
    pub async fn chunk<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.pagination_query(parent, &["*"])?
            .chunk(count, callback)
            .await
    }

    pub async fn each<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(R, crate::support::ArrayKey) -> Fut,
        count: u64,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.pagination_query(parent, &["*"])?
            .each(callback, count)
            .await
    }

    pub async fn eachById<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(R, i64) -> Fut,
        count: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.pagination_query(parent, &["*"])?
            .eachById(
                callback,
                count,
                column,
                alias.or(Some(R::metadata().primary_key)),
            )
            .await
    }

    super::key_chunks::key_chunks!(crate::database::eloquent::Collection<R>);

    pub async fn orderedChunkById<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.pagination_query(parent, &["*"])?
            .orderedChunkById(
                count,
                callback,
                column,
                alias.or(Some(R::metadata().primary_key)),
                descending,
            )
            .await
    }

    /// Streams related models across the constrained intermediate join.
    pub fn cursor<'a>(&'a self, parent: &'a P) -> DatabaseCursor<'a, R> {
        Box::pin(async_stream::try_stream! {
            let query = self.pagination_query(parent, &["*"])?;
            let mut models = query.cursor();
            while let Some(model) = models.try_next().await? {
                yield model;
            }
        })
    }

    fn pagination_query(
        &self,
        parent: &P,
        columns: &[&str],
    ) -> DatabaseResult<Builder<'static, R>> {
        let connection = self.keys.connection(parent)?;
        let query = R::queryOn(&connection);
        let projection = query.qualifyColumn("*");
        let query = if columns.is_empty() || columns == ["*"] {
            query.select_relation_columns([projection.as_str()])
        } else {
            let columns: Vec<_> = columns
                .iter()
                .map(|column| query.qualifyColumn(column))
                .collect();
            query.select_relation_columns(columns.iter().map(String::as_str))
        };
        let query = self.keys.configure(query)?;
        let join = self.join(&query, None, &connection)?;
        let query = self.select_one(query, &join, &connection)?;
        let keys = ParentKeys::new(&[parent], self.keys.parent)?;
        Ok(query
            .restrict_relation(&join.match_key, &keys.bindings)
            .map_relation_query(|query| join.apply(query)))
    }

    fn join(
        &self,
        related: &Builder<'_, R>,
        parent_table: Option<&str>,
        connection: &Connection,
    ) -> DatabaseResult<ThroughJoin> {
        let intermediate = I::metadata();
        if intermediate
            .connection
            .is_some_and(|name| name != connection.getName())
        {
            return Err(DatabaseError::InvalidQuery(
                "through models require one joined connection",
            ));
        }
        let related_table = related.table_name();
        let table = intermediate.table;
        let mut alias = table;
        if table == related_table || parent_table == Some(table) {
            alias = [
                "laravel_reserved_1",
                "laravel_reserved_2",
                "laravel_reserved_3",
            ]
            .into_iter()
            .find(|alias| *alias != related_table && Some(*alias) != parent_table)
            .expect("three aliases for two tables");
        }
        let source = if alias == table {
            table.into()
        } else {
            format!("{table} as {alias}")
        };
        Ok(ThroughJoin {
            source,
            alias,
            match_key: format!("{alias}.{}", self.first_key),
            intermediate_key: format!("{alias}.{}", self.second_local_key),
            related_key: related.qualifyColumn(self.keys.related),
            morph: self
                .intermediate_morph
                .map(|(column, value)| (format!("{alias}.{column}"), value)),
            deleted_at: intermediate
                .deleted_at
                .filter(|_| !self.include_trashed_parents)
                .map(|column| format!("{alias}.{column}")),
        })
    }

    fn select_one(
        &self,
        related: Builder<'static, R>,
        outer: &ThroughJoin,
        connection: &Connection,
    ) -> DatabaseResult<Builder<'static, R>> {
        let Some(selection) = &self.selection else {
            return Ok(related);
        };
        selection.apply_grouped(related, Some(outer.alias), |candidates| {
            let join = self.join(&candidates, Some(outer.alias), connection)?;
            let candidates = candidates.scoped()?.0;
            // Correlate through the intermediate key so ranking spans every
            // intermediate row of this parent. Candidate filters see R alone.
            let middle = candidates
                .subquery(&join.source)
                .select([query::Expression::new("1")])
                .whereColumn(&join.intermediate_key, "=", &join.related_key)
                .whereColumn(&join.match_key, "=", &outer.match_key);
            candidates.whereExists(&join.filter_intermediate(middle))
        })
    }

    pub(super) async fn contains_model(&self, parent: &P, related: &R) -> DatabaseResult<bool> {
        if !self.keys.shares_related_source(parent, related) {
            return Ok(false);
        }
        let (Some(parent_key), Some(related_key)) = (
            parent
                .getAttribute(self.keys.parent)
                .filter(|key| !matches!(key, Value::Null)),
            related.getKey().filter(|key| !matches!(key, Value::Null)),
        ) else {
            return Ok(false);
        };
        let connection = self.keys.connection(parent)?;
        let query = self.keys.configure(R::queryOn(&connection))?;
        let join = self.join(&query, None, &connection)?;
        let query = self.select_one(query, &join, &connection)?;
        let primary = query.qualifyColumn(R::metadata().primary_key);
        let query = query
            .restrict_relation(&join.match_key, std::slice::from_ref(parent_key))
            .restrict_relation(&primary, std::slice::from_ref(related_key));
        join.apply(query.scoped()?.0).exists().await
    }

    /// Writes the related table using the supplied intermediate foreign keys.
    pub async fn upsert(
        &self,
        parent: &P,
        rows: Vec<Attributes>,
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let connection = self.keys.connection(parent)?;
        self.keys
            .configure(R::queryOn(&connection))?
            .upsert(rows, unique_by, update)
            .await
    }

    /// Finds the related primary key through the join or returns an unsaved model.
    pub async fn findOrNew(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        match self.find(parent, key).await? {
            Some(model) => Ok(model),
            None => R::newInstanceOn(&self.keys.connection(parent)?, Attributes::new()),
        }
    }

    super::lookup::relation_lookup!();

    pub(super) async fn lookup_models(
        &self,
        parent: &P,
        keys: &Attributes,
        limit: u64,
    ) -> DatabaseResult<Vec<R>> {
        let lookup = ThroughLookup {
            selection: super::lookup::LookupSelection::limited(keys, limit),
            include_trashed: false,
        };
        let mut groups = self.match_many(&[parent], None, Some(lookup)).await?;
        Ok(groups
            .pop()
            .unwrap_or_default()
            .into_iter()
            .map(owned_model)
            .collect())
    }

    pub(super) async fn lookup_many(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        let lookup = ThroughLookup {
            selection: super::lookup::LookupSelection::Keys(keys),
            include_trashed: false,
        };
        let mut groups = self.match_many(&[parent], None, Some(lookup)).await?;
        let models = groups
            .pop()
            .unwrap_or_default()
            .into_iter()
            .map(owned_model)
            .collect();
        Ok(R::newCollection(models))
    }

    /// Finds a related row or makes an unsaved model using the supplied attributes.
    pub async fn firstOrNew(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        if let Some(model) = self.first_match(parent, &attributes, false).await? {
            return Ok(model);
        }
        let mut attributes = attributes;
        attributes.extend(values);
        let connection = self.keys.connection(parent)?;
        self.keys
            .configure(R::queryOn(&connection))?
            .makeAsync(attributes)
            .await
    }

    pub async fn firstOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.find_or_create(parent, attributes, values, false, false)
            .await
    }

    pub async fn createOrFirst(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        self.find_or_create(parent, attributes, values, false, true)
            .await
    }

    pub async fn updateOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        let mut model = self
            .firstOrCreate(parent, attributes, values.clone())
            .await?;
        if !model.state().wasRecentlyCreated() {
            model.fillAsync(values).await?;
            model.save().await?;
        }
        Ok(model)
    }

    pub async fn restoreOrCreate(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        super::require_soft_deletes::<R>()?;
        let mut model = self
            .find_or_create(parent, attributes, values, true, false)
            .await?;
        model.restore().await?;
        Ok(model)
    }

    pub async fn createOrRestore(
        &self,
        parent: &P,
        attributes: Attributes,
        values: Attributes,
    ) -> DatabaseResult<R> {
        super::require_soft_deletes::<R>()?;
        let mut model = self
            .find_or_create(parent, attributes, values, true, true)
            .await?;
        model.restore().await?;
        Ok(model)
    }

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
            .find_or_create(parent, attributes, values, false, false)
            .await?;
        if model.state().wasRecentlyCreated() {
            return Ok(model);
        }
        increment_related(model, column, step, extra).await
    }

    async fn find_or_create(
        &self,
        parent: &P,
        mut attributes: Attributes,
        values: Attributes,
        include_trashed: bool,
        insert_first: bool,
    ) -> DatabaseResult<R> {
        if parent
            .getAttribute(self.keys.parent)
            .is_none_or(|key| matches!(key, Value::Null))
        {
            return Err(DatabaseError::InvalidQuery("relationship parent key"));
        }
        if !insert_first {
            if let Some(model) = self
                .first_match(parent, &attributes, include_trashed)
                .await?
            {
                return Ok(model);
            }
            // Laravel's through firstOrCreate passes the merged attributes to createOrFirst.
            attributes.extend(values.clone());
        }
        let connection = self.keys.connection(parent)?;
        let mut insert = attributes.clone();
        insert.extend(values);
        let mut model = self
            .keys
            .configure(R::queryOn(&connection))?
            .makeAsync(insert)
            .await?;
        match model.save().await {
            Ok(true) => Ok(model),
            Ok(false) => Err(DatabaseError::InvalidQuery(
                "related model creation cancelled",
            )),
            Err(error) if super::super::persistence::is_unique_violation(&error) => self
                .first_match(parent, &attributes, include_trashed)
                .await?
                .ok_or(error),
            Err(error) => Err(error),
        }
    }

    async fn first_match(
        &self,
        parent: &P,
        attributes: &Attributes,
        include_trashed: bool,
    ) -> DatabaseResult<Option<R>> {
        Ok(self
            .match_many(
                &[parent],
                None,
                Some(ThroughLookup {
                    selection: super::lookup::LookupSelection::limited(attributes, 1),
                    include_trashed,
                }),
            )
            .await?
            .pop()
            .and_then(|matches| matches.into_iter().next())
            .map(owned_model))
    }

    async fn match_many(
        &self,
        parents: &[&P],
        mut transaction: Option<&mut Transaction>,
        lookup: Option<ThroughLookup<'_>>,
    ) -> DatabaseResult<Vec<Vec<Arc<R>>>> {
        let keys = ParentKeys::new(parents, self.keys.parent)?;
        let mut dictionary: HashMap<_, Vec<_>> = HashMap::new();
        if !keys.bindings.is_empty() {
            let connection = self.keys.batch_connection(parents)?;
            let projection = format!("{}.*", R::metadata().table);
            for batch in keys.bindings.chunks(500) {
                let query = self
                    .keys
                    .configure(
                        R::queryOn(&connection).select_relation_columns([projection.as_str()]),
                    )?
                    .apply_default_counts()?;
                let query = if lookup.as_ref().is_some_and(|lookup| lookup.include_trashed) {
                    query.withTrashed()
                } else {
                    query
                };
                let join = self.join(&query, None, &connection)?;
                let query = self.select_one(query, &join, &connection)?;
                let mut query = query.restrict_relation(&join.match_key, batch);
                if let Some(lookup) = &lookup {
                    query = lookup.selection.apply(query);
                }
                let eager_load = query.take_eager_load();
                let existence = query.take_existence_attributes();
                let casts = query.casts.clone();
                let match_column = format!("{} as laravel_through_key", join.match_key);
                let rows = join
                    .apply(query.scoped()?.0)
                    .addSelect([match_column.as_str()])
                    .get_with_transaction(transaction.as_deref_mut())
                    .await?;
                let mut matches = Vec::with_capacity(rows.len());
                let mut related = Vec::with_capacity(rows.len());
                for row in rows {
                    let mut columns = row.into_columns();
                    let (_, key) = columns
                        .pop()
                        .ok_or(DatabaseError::InvalidQuery("through match key"))?;
                    if let Some(key) = dictionary_key(Some(&key))? {
                        matches.push(key);
                        let mut attributes = columns.into_iter().collect();
                        cast_existence(&mut attributes, &existence);
                        related.push(R::from_state(ModelState::hydrated_attributes(
                            attributes,
                            connection.clone(),
                            casts.clone(),
                        )));
                    }
                }
                super::super::concerns::has_events::retrieved_with_transaction(
                    &mut related,
                    &connection,
                    transaction
                        .as_deref_mut()
                        .filter(|tx| tx.belongs_to(&connection)),
                )
                .await?;
                load_eager_with_transaction(eager_load, &mut related, transaction.as_deref_mut())
                    .await?;
                for (key, model) in matches.into_iter().zip(related) {
                    dictionary.entry(key).or_default().push(Arc::new(model));
                }
            }
        }
        Ok(keys.match_models(&dictionary))
    }
}

struct ThroughJoin {
    source: String,
    alias: &'static str,
    match_key: String,
    intermediate_key: String,
    related_key: String,
    deleted_at: Option<String>,
    morph: Option<(String, &'static str)>,
}

impl ThroughJoin {
    fn apply(&self, query: query::Builder<'static>) -> query::Builder<'static> {
        let query = query.join(&self.source, &self.intermediate_key, "=", &self.related_key);
        self.filter_intermediate(query)
    }

    fn filter_intermediate(&self, mut query: query::Builder<'static>) -> query::Builder<'static> {
        if let Some((column, value)) = &self.morph {
            query = query.r#where(column, "=", *value);
        }
        match &self.deleted_at {
            Some(column) => query.whereNull(column),
            None => query,
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model, I: Model, R: Model> Relation for HasManyThrough<P, I, R> {
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

impl<P: Model, I: Model, R: Model> HasManyThrough<P, I, R> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        Box::pin(async move {
            let results = self.match_many(parents, transaction, None).await?;
            Ok(results
                .into_iter()
                .map(super::super::RelatedCollection::from_models)
                .collect())
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, I: Model, R: Model> ExistenceRelation for HasManyThrough<P, I, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        let related = self.keys.existence_builder(connection, parent_table)?;
        let join = self.join(&related, Some(parent_table), connection)?;
        let related = self.select_one(related, &join, connection)?;
        let parent = format!("{parent_table}.{}", self.keys.parent);
        Ok(join
            .apply(related.scoped()?.0)
            .whereColumn(&join.match_key, "=", &parent))
    }
}

impl<P: Model, I: Model, R: Model> super::NestedRelation for HasManyThrough<P, I, R> {
    type Related = R;

    fn with_nested(mut self, nested: super::super::EagerLoads<R>) -> Self {
        self.keys.with_nested(nested);
        self
    }

    fn with_path_columns(mut self, columns: Vec<String>) -> DatabaseResult<Self> {
        self.keys
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

impl<P: Model, I: Model, R: Model> super::ConstrainableRelation for HasManyThrough<P, I, R> {
    type Related = R;

    fn constrain_query(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.keys.append_constraint(configure);
        self
    }
}
