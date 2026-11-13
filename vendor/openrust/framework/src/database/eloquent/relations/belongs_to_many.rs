use std::{collections::HashMap, sync::Arc};

use indexmap::IndexMap;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, Model, ModelState,
        concerns::{
            has_relationships::{LoadedRelations, load_eager},
            has_timestamps::timestamp,
            queries_relationships::cast_existence,
        },
    },
    query,
};

use super::{
    ExistenceRelation, Pivot, Relation,
    custom_pivot::{CustomPivot, PivotIdentity},
    pivot_constraints::{PivotConstraints, pivot_predicates},
    relation::{ParentKeys, RelationKeys, dictionary_key},
};

/// Links two models through an intermediate table.
pub struct BelongsToMany<P: Model, R: Model> {
    keys: RelationKeys<P, R>,
    pivot: PivotKeys,
    pivot_columns: Vec<&'static str>,
    pivot_accessor: &'static str,
    pivot_constraints: PivotConstraints,
    pivot_orders: Vec<(String, String)>,
    pivot_timestamps: Option<PivotTimestampColumns>,
    custom_pivot: Option<CustomPivot>,
    pivot_inverses: (Option<String>, Option<String>),
}

/// Keys attached, detached, or updated by a pivot synchronization.
#[derive(Debug, PartialEq, Default)]
pub struct SyncChanges {
    pub attached: Vec<Value>,
    pub detached: Vec<Value>,
    pub updated: Vec<Value>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> BelongsToMany<P, R> {
    pub const fn new(
        table: &'static str,
        foreign_pivot_key: &'static str,
        related_pivot_key: &'static str,
        parent_key: &'static str,
        related_key: &'static str,
    ) -> Self {
        Self {
            keys: RelationKeys::new(parent_key, related_key),
            pivot: PivotKeys {
                table,
                foreign: foreign_pivot_key,
                related: related_pivot_key,
                morph: None,
            },
            pivot_columns: Vec::new(),
            pivot_accessor: "pivot",
            pivot_constraints: PivotConstraints::new(),
            pivot_orders: Vec::new(),
            pivot_timestamps: None,
            custom_pivot: None,
            pivot_inverses: (None, None),
        }
    }

    pub(super) const fn morph(mut self, column: &'static str, alias: &'static str) -> Self {
        self.pivot.morph = Some((column, alias));
        self
    }

    /// Applies constraints and nested eager loads to the related query.
    #[must_use]
    pub fn constrain(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.keys.constrain(configure);
        self
    }

    #[must_use]
    pub fn withPivot(mut self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        for column in columns {
            if !matches!(column, "*")
                && !self.pivot_columns.contains(&column)
                && column != self.pivot.foreign
                && column != self.pivot.related
                && self.pivot.morph.is_none_or(|(kind, _)| column != kind)
            {
                self.pivot_columns.push(column);
            }
        }
        self
    }

    #[must_use]
    pub fn r#as(mut self, name: &'static str) -> Self {
        self.pivot_accessor = name;
        self
    }

    /// Orders related rows by a column of the intermediate table.
    #[must_use]
    pub fn orderByPivot(mut self, column: &str, direction: &str) -> Self {
        let column = if column.contains('.') {
            column.to_owned()
        } else {
            format!("{}.{column}", self.pivot.table)
        };
        self.pivot_orders.push((column, direction.into()));
        self
    }

    #[must_use]
    pub fn orderByPivotDesc(self, column: &str) -> Self {
        self.orderByPivot(column, "desc")
    }

    /// Selects and maintains the parent's conventional timestamp columns on the pivot.
    #[must_use]
    pub fn withTimestamps(self) -> Self {
        let (created, updated) = Self::default_timestamp_columns();
        self.with_timestamp_columns((Some(created), Some(updated)))
    }

    /// Sets explicit timestamp columns. A `None` skips that column.
    pub fn withTimestampColumns(
        self,
        created: Option<&'static str>,
        updated: Option<&'static str>,
    ) -> DatabaseResult<Self> {
        let columns = Attributes::from_iter(
            created
                .into_iter()
                .chain(updated)
                .map(|column| (column.into(), Value::Null)),
        );
        if columns
            .keys()
            .any(|name| name.is_empty() || name.contains('.'))
            || matches!((created, updated), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b))
        {
            return Err(DatabaseError::InvalidQuery(
                "invalid pivot timestamp columns",
            ));
        }
        self.validate_pivot_attributes(&columns)?;
        Ok(self.with_timestamp_columns((created, updated)))
    }

    fn with_timestamp_columns(mut self, columns: PivotTimestampColumns) -> Self {
        self.pivot_timestamps = Some(columns);
        self
    }

    fn default_timestamp_columns() -> (&'static str, &'static str) {
        let metadata = P::metadata();
        (
            metadata.created_at.unwrap_or("created_at"),
            metadata.updated_at.unwrap_or("updated_at"),
        )
    }

    fn timestamp_columns(&self) -> PivotTimestampColumns {
        self.pivot_timestamps.unwrap_or_else(|| {
            let (created, updated) = Self::default_timestamp_columns();
            (
                self.pivot_columns.contains(&created).then_some(created),
                self.pivot_columns.contains(&updated).then_some(updated),
            )
        })
    }

    fn pivot_write_timestamps(
        &self,
        connection: &Connection,
    ) -> DatabaseResult<PivotWriteTimestamps> {
        let (created, updated) = self.timestamp_columns();
        let mut insert = Attributes::new();
        let mut update = Attributes::new();
        if created.is_some() || updated.is_some() {
            let now = timestamp(connection, P::metadata().date_format)?;
            if let Some(column) = created {
                insert.insert(column.into(), now.clone());
            }
            if let Some(column) = updated {
                insert.insert(column.into(), now.clone());
                update.insert(column.into(), now);
            }
            self.validate_pivot_attributes(&insert)?;
        }
        Ok(PivotWriteTimestamps { insert, update })
    }

    /// Hydrates and persists links using the declared model.
    #[must_use]
    pub fn using<M: Model>(mut self) -> Self {
        self.custom_pivot = Some(CustomPivot::new::<M>());
        self.pivot_inverses = (None, None);
        self
    }

    /// Hydrates declared inverse relations from the models already participating in this load.
    pub fn chaperone(self) -> Self {
        self.chaperoneWith(None, None)
            .expect("inferred names cannot fail validation")
    }

    /// Uses explicit inverse names for either endpoint and infers omitted names.
    pub fn chaperoneWith(
        mut self,
        declaring: Option<&str>,
        related: Option<&str>,
    ) -> DatabaseResult<Self> {
        if let Some(custom) = self.custom_pivot {
            self.pivot_inverses = (
                pivot_inverse::<P>(custom, declaring, self.pivot.foreign)?,
                pivot_inverse::<R>(custom, related, self.pivot.related)?,
            );
        }
        Ok(self)
    }

    #[must_use]
    pub fn withoutChaperone(mut self) -> Self {
        self.pivot_inverses = (None, None);
        self
    }

    pivot_predicates!(apply_pivot_predicate);

    pub(super) fn apply_pivot_predicate(
        mut self,
        column: &str,
        predicate: impl FnOnce(query::Conditions, &str) -> query::Conditions,
    ) -> Self {
        self.pivot_constraints
            .add(self.pivot.table, column, predicate);
        self
    }

    /// Filters the relation and supplies the value when inserting new links.
    pub fn withPivotValue(mut self, column: &str, value: impl Into<Value>) -> DatabaseResult<Self> {
        let value = value.into();
        if matches!(value, Value::Null) || column.is_empty() || column.contains('.') {
            return Err(DatabaseError::InvalidQuery(
                "pivot default requires a column and a non-null value",
            ));
        }
        self.validate_pivot_attributes(&Attributes::from([(column.into(), value.clone())]))?;
        self.pivot_constraints
            .defaults
            .insert(column.into(), value.clone());
        Ok(self.wherePivot(column, "=", value))
    }

    pub async fn attach(&self, parent: &P, keys: &[Value]) -> DatabaseResult<()> {
        self.attachWith(parent, keys, Attributes::new()).await
    }

    pub fn attachWith<'a>(
        &'a self,
        parent: &'a P,
        keys: &'a [Value],
        attributes: Attributes,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            self.validate_pivot_attributes(&attributes)?;
            let owner = self.owner(parent)?;
            let keys = key_map(keys.iter().cloned())?;
            let connection = self.keys.connection(parent)?;
            let timestamps = self.pivot_write_timestamps(&connection)?;
            let mut rows = self.pivot.records(owner, keys.values());
            for row in &mut rows {
                row.extend(timestamps.insert.clone());
                row.extend(self.pivot_constraints.defaults.clone());
                row.extend(attributes.clone());
            }
            if let Some(custom) = self.custom_pivot {
                let identity = self.pivot.identity(self.timestamp_columns());
                let source = connection.clone();
                let rows = Arc::new(rows);
                connection
                    .transaction(
                        move |transaction| {
                            let source = source.clone();
                            let rows = rows.clone();
                            Box::pin(async move {
                                for row in rows.iter() {
                                    (custom.insert)(transaction, &source, identity, row.clone())
                                        .await?;
                                }
                                Ok(())
                            })
                        },
                        3,
                    )
                    .await?;
            } else {
                connection.table(self.pivot.table).insert(&rows).await?;
            }
            Ok(())
        })
    }

    pub fn updateExistingPivot<'a>(
        &'a self,
        parent: &'a P,
        related: &'a Value,
        mut attributes: Attributes,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(async move {
            self.validate_pivot_attributes(&attributes)?;
            let owner = self.owner(parent)?;
            if dictionary_key(Some(related))?.is_none() {
                return Err(DatabaseError::InvalidQuery("pivot key is null"));
            }
            let (_, updated) = self.timestamp_columns();
            if attributes.is_empty() && (updated.is_none() || self.custom_pivot.is_some()) {
                return Ok(0);
            }
            let connection = self.keys.connection(parent)?;
            if let Some(custom) = self.custom_pivot {
                let pivot = self.pivot;
                let identity = pivot.identity(self.timestamp_columns());
                let owner = owner.clone();
                let related = related.clone();
                let constraints = self.pivot_constraints.clone();
                let source = connection.clone();
                return connection
                    .transaction(
                        move |transaction| {
                            let owner = owner.clone();
                            let related = related.clone();
                            let constraints = constraints.clone();
                            let source = source.clone();
                            let attributes = attributes.clone();
                            Box::pin(async move {
                                let raw = constraints
                                    .apply(pivot.for_owner(transaction.table(pivot.table), &owner))
                                    .r#where(pivot.related, "=", related)
                                    .lockForUpdate()
                                    .first()
                                    .await?;
                                if let Some(raw) = raw {
                                    return (custom.update)(
                                        transaction,
                                        &source,
                                        identity,
                                        raw.into_columns().into_iter().collect(),
                                        attributes,
                                    )
                                    .await
                                    .map(u64::from);
                                }
                                Ok(0)
                            })
                        },
                        3,
                    )
                    .await;
            }
            if let Some(column) = updated {
                attributes.retain(|name, _| !name.eq_ignore_ascii_case(column));
                attributes.extend(self.pivot_write_timestamps(&connection)?.update);
            }
            let query = connection.table(self.pivot.table);
            self.pivot_constraints
                .apply(self.pivot.for_owner(query, owner))
                .r#where(self.pivot.related, "=", related.clone())
                .update(attributes)
                .await
        })
    }

    fn validate_pivot_attributes(&self, attributes: &Attributes) -> DatabaseResult<()> {
        let protected = [self.pivot.foreign, self.pivot.related]
            .into_iter()
            .chain(self.pivot.morph.map(|(column, _)| column));
        if attributes.keys().any(|column| {
            protected
                .clone()
                .any(|key| column.eq_ignore_ascii_case(key))
        }) {
            return Err(DatabaseError::InvalidQuery(
                "pivot attributes cannot replace relationship keys",
            ));
        }
        Ok(())
    }

    pub fn detach<'a>(
        &'a self,
        parent: &'a P,
        keys: Option<&'a [Value]>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(async move {
            let owner = self.owner(parent)?;
            if let Some(custom) = self.custom_pivot {
                let pivot = self.pivot;
                let identity = pivot.identity(self.timestamp_columns());
                let constraints = self.pivot_constraints.clone();
                let owner = owner.clone();
                let keys = keys
                    .map(|keys| {
                        key_map(keys.iter().cloned())
                            .map(|keys| keys.into_values().collect::<Vec<_>>())
                    })
                    .transpose()?;
                let connection = self.keys.connection(parent)?;
                let source = connection.clone();
                return connection
                    .transaction(
                        move |transaction| {
                            let source = source.clone();
                            let constraints = constraints.clone();
                            let keys = keys.clone();
                            let owner = owner.clone();
                            Box::pin(async move {
                                let mut query = constraints
                                    .apply(pivot.for_owner(transaction.table(pivot.table), &owner));
                                if let Some(keys) = keys {
                                    query = query.whereIn(pivot.related, keys);
                                }
                                let rows = query.lockForUpdate().get().await?;
                                let mut removed = 0;
                                for row in rows {
                                    removed += u64::from(
                                        (custom.delete)(
                                            transaction,
                                            &source,
                                            identity,
                                            row.into_columns().into_iter().collect(),
                                        )
                                        .await?,
                                    );
                                }
                                Ok(removed)
                            })
                        },
                        3,
                    )
                    .await;
            }
            let query = self.keys.connection(parent)?.table(self.pivot.table);
            let mut query = self
                .pivot_constraints
                .apply(self.pivot.for_owner(query, owner));
            if let Some(keys) = keys {
                let keys = key_map(keys.iter().cloned())?;
                query = query.whereIn(self.pivot.related, keys.into_values().collect());
            }
            query.delete().await
        })
    }

    pub async fn sync(&self, parent: &P, keys: &[Value]) -> DatabaseResult<SyncChanges> {
        self.syncWithPivotValues(parent, keys, Attributes::new(), true)
            .await
    }

    pub async fn syncWithoutDetaching(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> DatabaseResult<SyncChanges> {
        self.syncWithPivotValues(parent, keys, Attributes::new(), false)
            .await
    }

    pub async fn syncWithPivotValues(
        &self,
        parent: &P,
        keys: &[Value],
        attributes: Attributes,
        detach: bool,
    ) -> DatabaseResult<SyncChanges> {
        self.synchronize(
            parent,
            keys.iter().cloned().map(|key| (key, attributes.clone())),
            detach,
        )
        .await
    }

    /// Synchronizes related keys with a distinct set of pivot values for each key.
    pub async fn syncWithPivotAttributes(
        &self,
        parent: &P,
        records: &[(Value, Attributes)],
        detach: bool,
    ) -> DatabaseResult<SyncChanges> {
        self.synchronize(parent, records.iter().cloned(), detach)
            .await
    }

    fn synchronize<'a>(
        &'a self,
        parent: &'a P,
        records: impl Iterator<Item = (Value, Attributes)> + Send + 'a,
        detach: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<SyncChanges>> {
        Box::pin(async move {
            let owner = self.owner(parent)?.clone();
            let desired = records
                .map(|(key, attributes)| {
                    self.validate_pivot_attributes(&attributes)?;
                    let normalized = dictionary_key(Some(&key))?
                        .ok_or(DatabaseError::InvalidQuery("pivot key is null"))?;
                    Ok((normalized, (key, attributes)))
                })
                .collect::<DatabaseResult<DesiredPivots>>()?;
            if desired.is_empty() && !detach {
                return Ok(SyncChanges::default());
            }
            let desired = Arc::new(desired);
            let pivot = self.pivot;
            let constraints = Arc::new(self.pivot_constraints.clone());
            let connection = self.keys.connection(parent)?;
            let timestamps = Arc::new(self.pivot_write_timestamps(&connection)?);
            let custom = self.custom_pivot;
            let identity = pivot.identity(self.timestamp_columns());
            let source = connection.clone();
            connection
                .transaction(
                    move |transaction| {
                        if let Some(custom) = custom {
                            return Box::pin(sync_custom_pivot(
                                transaction,
                                CustomSync {
                                    pivot,
                                    custom,
                                    identity,
                                    source: source.clone(),
                                    owner: owner.clone(),
                                    desired: desired.clone(),
                                    constraints: constraints.clone(),
                                    timestamps: timestamps.clone(),
                                    detach,
                                },
                            ));
                        }
                        Box::pin(sync_pivot(
                            transaction,
                            pivot,
                            owner.clone(),
                            desired.clone(),
                            constraints.clone(),
                            timestamps.clone(),
                            detach,
                        ))
                    },
                    3,
                )
                .await
        })
    }

    fn owner<'a>(&self, parent: &'a P) -> DatabaseResult<&'a Value> {
        parent
            .getAttribute(self.keys.parent)
            .filter(|value| !matches!(value, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship parent key"))
    }

    async fn match_many(&self, parents: &[&P]) -> DatabaseResult<Vec<Vec<Arc<R>>>> {
        let keys = ParentKeys::new(parents, self.keys.parent)?;
        let mut dictionary: HashMap<_, Vec<_>> = HashMap::new();
        if !keys.bindings.is_empty() {
            let connection = self.keys.batch_connection(parents)?;
            let (declaring_inverse, related_inverse) = &self.pivot_inverses;
            let mut declaring_models = HashMap::new();
            if declaring_inverse.is_some() {
                for parent in parents {
                    if let Some(key) = dictionary_key(parent.getAttribute(self.keys.parent))? {
                        declaring_models
                            .entry(key)
                            .or_insert_with(|| Arc::new(P::from_state(parent.state().clone())));
                    }
                }
            }
            let parent_column = format!("{}.{}", self.pivot.table, self.pivot.foreign);
            let pivot_key = format!("{}.{}", self.pivot.table, self.pivot.related);
            let related_key = format!("{}.{}", R::metadata().table, self.keys.related);
            let related_columns = format!("{}.*", R::metadata().table);
            let mut pivot_names: Vec<_> = [self.pivot.foreign, self.pivot.related]
                .into_iter()
                .chain(self.pivot.morph.map(|(kind, _)| kind))
                .chain(self.pivot_columns.iter().copied())
                .collect();
            if let Some((created, updated)) = self.pivot_timestamps {
                for column in created.into_iter().chain(updated) {
                    if !pivot_names.contains(&column) {
                        pivot_names.push(column);
                    }
                }
            }
            if let Some(primary_key) = self.custom_pivot.and_then(|custom| custom.primary_key)
                && !pivot_names.contains(&primary_key)
            {
                pivot_names.push(primary_key);
            }
            let pivot_columns: Vec<_> = pivot_names
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    format!("{}.{column} as laravel_pivot_{index}", self.pivot.table)
                })
                .collect();
            for batch in keys.bindings.chunks(500) {
                let mut query = self
                    .keys
                    .configure(
                        R::queryOn(&connection).select_relation_columns([related_columns.as_str()]),
                    )?
                    .apply_default_counts()?
                    .restrict_relation(&parent_column, batch);
                let eager_load = query.take_eager_load();
                let existence = query.take_existence_attributes();
                let casts = query.casts.clone();
                let query = query
                    .scoped()?
                    .0
                    .join(self.pivot.table, &related_key, "=", &pivot_key);
                let query = self
                    .pivot_constraints
                    .apply(self.pivot.restrict_type(query, true));
                let query = self
                    .pivot_orders
                    .iter()
                    .fold(query, |query, (column, direction)| {
                        query.orderBy(column, direction)
                    });
                let rows = query
                    .addSelect(pivot_columns.iter().map(String::as_str))
                    .get()
                    .await?;
                let mut models = Vec::with_capacity(rows.len());
                let mut owners = Vec::with_capacity(rows.len());
                for row in rows {
                    let mut columns = row.into_columns();
                    let pivot_start = columns
                        .len()
                        .checked_sub(pivot_names.len())
                        .ok_or(DatabaseError::InvalidQuery("pivot match key"))?;
                    let pivot = columns.split_off(pivot_start);
                    if let Some(owner) = dictionary_key(Some(&pivot[0].1))? {
                        let mut model_attributes = columns.into_iter().collect();
                        cast_existence(&mut model_attributes, &existence);
                        let mut state = ModelState::hydrated_attributes(
                            model_attributes,
                            connection.clone(),
                            casts.clone(),
                        );
                        let attributes = pivot_names
                            .iter()
                            .zip(pivot)
                            .map(|(name, (_, value))| ((*name).into(), value))
                            .collect();
                        if let Some(custom) = self.custom_pivot {
                            let mut defaults = self.pivot_constraints.defaults.clone();
                            defaults.extend(attributes);
                            let mut inverses = LoadedRelations::default();
                            if let Some(name) = declaring_inverse {
                                inverses.insert(
                                    name.clone(),
                                    declaring_models.get(&owner).cloned(),
                                    super::super::serialization::one::<P>,
                                );
                            }
                            if let Some(name) = related_inverse {
                                // An owned snapshot avoids a strong reference cycle through the pivot.
                                let related = Arc::new(R::from_state(state.clone()));
                                inverses.insert(
                                    name.clone(),
                                    Some(related),
                                    super::super::serialization::one::<R>,
                                );
                            }
                            (custom.hydrate)(
                                &mut state.relations,
                                self.pivot_accessor,
                                defaults,
                                &connection,
                                self.pivot.identity(self.timestamp_columns()),
                                inverses,
                            )?;
                        } else {
                            state.relations.insert(
                                self.pivot_accessor.into(),
                                Pivot::new(attributes),
                                Pivot::serialize,
                            );
                        }
                        owners.push(owner);
                        models.push(R::from_state(state));
                    }
                }
                super::super::concerns::has_events::retrieved(&mut models, &connection).await?;
                load_eager(eager_load, &mut models).await?;
                for (owner, model) in owners.into_iter().zip(models) {
                    dictionary.entry(owner).or_default().push(Arc::new(model));
                }
            }
        }
        Ok(keys.match_models(&dictionary))
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
impl<P: Model, R: Model> Relation for BelongsToMany<P, R> {
    type Parent = P;
    type Output = Vec<Arc<R>>;

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::many(output)
    }

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(self.match_many(parents))
    }
}

fn pivot_inverse<M: Model>(
    pivot: CustomPivot,
    explicit: Option<&str>,
    foreign_key: &str,
) -> DatabaseResult<Option<String>> {
    use crate::support::Str;

    if let Some(name) = explicit {
        if !pivot.inverse_relations.contains(&name) {
            return Err(DatabaseError::InvalidQuery(
                "unknown custom pivot inverse relation",
            ));
        }
        return Ok(Some(name.to_owned()));
    }
    let stem = foreign_key
        .rsplit_once(M::metadata().primary_key)
        .map_or(foreign_key, |(stem, _)| stem);
    let model_name = std::any::type_name::<M>()
        .split('<')
        .next()
        .unwrap_or_default()
        .rsplit("::")
        .next()
        .unwrap_or_default();
    Ok([Str::camel(stem), Str::camel(model_name)]
        .into_iter()
        .find(|candidate| {
            !candidate.is_empty() && pivot.inverse_relations.contains(&candidate.as_str())
        }))
}

fn key_map(keys: impl IntoIterator<Item = Value>) -> DatabaseResult<IndexMap<Vec<u8>, Value>> {
    keys.into_iter()
        .map(|key| {
            let normalized = dictionary_key(Some(&key))?
                .ok_or(DatabaseError::InvalidQuery("pivot key is null"))?;
            Ok((normalized, key))
        })
        .collect()
}

#[derive(Clone, Copy)]
struct PivotKeys {
    table: &'static str,
    foreign: &'static str,
    related: &'static str,
    morph: Option<(&'static str, &'static str)>,
}

impl PivotKeys {
    fn identity(self, timestamps: PivotTimestampColumns) -> PivotIdentity {
        PivotIdentity {
            table: self.table,
            foreign: self.foreign,
            related: self.related,
            morph: self.morph.map(|(column, _)| column),
            timestamps,
        }
    }

    fn restrict_type<'a>(&self, query: query::Builder<'a>, qualified: bool) -> query::Builder<'a> {
        match self.morph {
            Some((column, alias)) if qualified => {
                query.r#where(&format!("{}.{column}", self.table), "=", alias)
            }
            Some((column, alias)) => query.r#where(column, "=", alias),
            None => query,
        }
    }

    fn for_owner<'a>(&self, query: query::Builder<'a>, owner: &Value) -> query::Builder<'a> {
        self.restrict_type(query.r#where(self.foreign, "=", owner.clone()), false)
    }

    fn record(&self, owner: &Value, key: &Value) -> Attributes {
        let mut attributes = Attributes::from([
            (self.foreign.into(), owner.clone()),
            (self.related.into(), key.clone()),
        ]);
        if let Some((column, alias)) = self.morph {
            attributes.insert(column.into(), alias.into());
        }
        attributes
    }

    fn records<'a>(&self, owner: &Value, keys: impl Iterator<Item = &'a Value>) -> Vec<Attributes> {
        keys.map(|key| self.record(owner, key)).collect()
    }
}

type PivotTimestampColumns = (Option<&'static str>, Option<&'static str>);

struct PivotWriteTimestamps {
    insert: Attributes,
    update: Attributes,
}

type DesiredPivots = IndexMap<Vec<u8>, (Value, Attributes)>;

struct CustomSync {
    pivot: PivotKeys,
    custom: CustomPivot,
    identity: PivotIdentity,
    source: Connection,
    owner: Value,
    desired: Arc<DesiredPivots>,
    constraints: Arc<PivotConstraints>,
    timestamps: Arc<PivotWriteTimestamps>,
    detach: bool,
}

async fn sync_custom_pivot(
    transaction: &mut Transaction,
    sync: CustomSync,
) -> DatabaseResult<SyncChanges> {
    let CustomSync {
        pivot,
        custom,
        identity,
        source,
        owner,
        desired,
        constraints,
        timestamps,
        detach,
    } = sync;
    let rows = constraints
        .apply(pivot.for_owner(transaction.table(pivot.table), &owner))
        .lockForUpdate()
        .get()
        .await?;
    let mut current = IndexMap::new();
    let mut changes = SyncChanges::default();
    for row in rows {
        let raw: Attributes = row.into_columns().into_iter().collect();
        let key = raw
            .get(pivot.related)
            .ok_or(DatabaseError::InvalidQuery("missing related pivot key"))?
            .clone();
        let normalized =
            dictionary_key(Some(&key))?.ok_or(DatabaseError::InvalidQuery("pivot key is null"))?;
        if detach && !desired.contains_key(&normalized) {
            if (custom.delete)(transaction, &source, identity, raw).await? {
                changes.detached.push(key);
            }
        } else {
            current.insert(normalized, raw);
        }
    }
    for (normalized, (key, attributes)) in desired.iter() {
        if let Some(raw) = current.shift_remove(normalized) {
            if !attributes.is_empty()
                && (custom.update)(transaction, &source, identity, raw, attributes.clone()).await?
            {
                changes.updated.push(key.clone());
            }
        } else {
            let mut row = pivot.record(&owner, key);
            row.extend(timestamps.insert.clone());
            row.extend(constraints.defaults.clone());
            row.extend(attributes.clone());
            if (custom.insert)(transaction, &source, identity, row).await? {
                changes.attached.push(key.clone());
            }
        }
    }
    Ok(changes)
}

async fn sync_pivot(
    transaction: &mut Transaction,
    pivot: PivotKeys,
    owner: Value,
    desired: Arc<DesiredPivots>,
    constraints: Arc<PivotConstraints>,
    timestamps: Arc<PivotWriteTimestamps>,
    detach: bool,
) -> DatabaseResult<SyncChanges> {
    let query = transaction.table(pivot.table).select([pivot.related]);
    let current = constraints
        .apply(pivot.for_owner(query, &owner))
        .lockForUpdate()
        .get()
        .await?;
    let current = key_map(
        current
            .into_iter()
            .map(|row| row.into_columns().pop().expect("selected pivot key").1),
    )?;
    let detached: Vec<_> = current
        .iter()
        .filter(|(key, _)| detach && !desired.contains_key(*key))
        .map(|(_, value)| value.clone())
        .collect();
    for keys in detached.chunks(500) {
        let query = transaction.table(pivot.table);
        constraints
            .apply(pivot.for_owner(query, &owner))
            .whereIn(pivot.related, keys.to_vec())
            .delete()
            .await?;
    }
    let mut inserts: IndexMap<Vec<String>, Vec<Attributes>> = IndexMap::new();
    let mut changes = SyncChanges {
        detached,
        ..SyncChanges::default()
    };
    for (normalized, (key, attributes)) in desired.iter() {
        if !current.contains_key(normalized) {
            let mut row = pivot.record(&owner, key);
            row.extend(timestamps.insert.clone());
            row.extend(constraints.defaults.clone());
            row.extend(attributes.clone());
            inserts
                .entry(row.keys().cloned().collect())
                .or_default()
                .push(row);
            changes.attached.push(key.clone());
        } else if !attributes.is_empty() {
            let mut attributes = attributes.clone();
            attributes.retain(|name, _| {
                !timestamps
                    .update
                    .keys()
                    .any(|column| name.eq_ignore_ascii_case(column))
            });
            attributes.extend(timestamps.update.clone());
            let query = transaction.table(pivot.table);
            let affected = constraints
                .apply(pivot.for_owner(query, &owner))
                .r#where(pivot.related, "=", key.clone())
                .whereNot(|conditions| {
                    attributes
                        .iter()
                        .fold(conditions, |conditions, (column, value)| {
                            conditions.whereNullSafeEquals(column, value.clone())
                        })
                })
                .update(attributes)
                .await?;
            if affected > 0 {
                changes.updated.push(key.clone());
            }
        }
    }
    for rows in inserts.into_values() {
        for batch in rows.chunks(500) {
            transaction.table(pivot.table).insert(batch).await?;
        }
    }
    Ok(changes)
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, R: Model> ExistenceRelation for BelongsToMany<P, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.existence_query(connection, parent_table, None)
    }
}

impl<P: Model, R: Model> BelongsToMany<P, R> {
    pub(in crate::database::eloquent) fn existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
        target_keys: Option<Vec<Value>>,
    ) -> DatabaseResult<query::Builder<'static>> {
        let query = self.keys.existence_builder(connection, parent_table)?;
        let target_key = query.qualifyColumn(R::metadata().primary_key);
        let related = query.qualifyColumn(self.keys.related);
        let related_pivot = format!("{}.{}", self.pivot.table, self.pivot.related);
        let foreign_pivot = format!("{}.{}", self.pivot.table, self.pivot.foreign);
        let parent = format!("{parent_table}.{}", self.keys.parent);
        let query = query
            .scoped()?
            .0
            .join(self.pivot.table, &related, "=", &related_pivot)
            .whereColumn(&foreign_pivot, "=", &parent);
        let query = self
            .pivot_constraints
            .apply(self.pivot.restrict_type(query, true));
        Ok(match target_keys {
            Some(keys) => query.whereIn(&target_key, keys),
            None => query,
        })
    }
}
