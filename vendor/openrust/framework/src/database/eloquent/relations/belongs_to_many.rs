use std::{collections::HashMap, sync::Arc};

use indexmap::IndexMap;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, Model, ModelState,
        concerns::{has_relationships::load_eager, queries_relationships::cast_existence},
    },
    query,
};

use super::{
    ExistenceRelation, Pivot, Relation,
    relation::{ParentKeys, RelationKeys, dictionary_key},
};

/// Links two models through an intermediate table.
pub struct BelongsToMany<P: Model, R: Model> {
    keys: RelationKeys<P, R>,
    pivot: PivotKeys,
    pivot_columns: Vec<&'static str>,
    pivot_accessor: &'static str,
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

    pub async fn attach(&self, parent: &P, keys: &[Value]) -> DatabaseResult<()> {
        self.attachWith(parent, keys, Attributes::new()).await
    }

    pub async fn attachWith(
        &self,
        parent: &P,
        keys: &[Value],
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        self.validate_pivot_attributes(&attributes)?;
        let owner = self.owner(parent)?;
        let keys = key_map(keys.iter().cloned())?;
        let mut rows = self.pivot.records(owner, keys.values());
        for row in &mut rows {
            row.extend(attributes.clone());
        }
        self.keys
            .connection(parent)?
            .table(self.pivot.table)
            .insert(&rows)
            .await?;
        Ok(())
    }

    pub async fn updateExistingPivot(
        &self,
        parent: &P,
        related: &Value,
        attributes: Attributes,
    ) -> DatabaseResult<u64> {
        self.validate_pivot_attributes(&attributes)?;
        let owner = self.owner(parent)?;
        if dictionary_key(Some(related))?.is_none() {
            return Err(DatabaseError::InvalidQuery("pivot key is null"));
        }
        if attributes.is_empty() {
            return Ok(0);
        }
        let query = self.keys.connection(parent)?.table(self.pivot.table);
        self.pivot
            .for_owner(query, owner)
            .r#where(self.pivot.related, "=", related.clone())
            .update(attributes)
            .await
    }

    fn validate_pivot_attributes(&self, attributes: &Attributes) -> DatabaseResult<()> {
        if attributes.contains_key(self.pivot.foreign)
            || attributes.contains_key(self.pivot.related)
            || self
                .pivot
                .morph
                .is_some_and(|(kind, _)| attributes.contains_key(kind))
        {
            return Err(DatabaseError::InvalidQuery(
                "pivot attributes cannot replace relationship keys",
            ));
        }
        Ok(())
    }

    pub async fn detach(&self, parent: &P, keys: Option<&[Value]>) -> DatabaseResult<u64> {
        let owner = self.owner(parent)?;
        let query = self.keys.connection(parent)?.table(self.pivot.table);
        let mut query = self.pivot.for_owner(query, owner);
        if let Some(keys) = keys {
            let keys = key_map(keys.iter().cloned())?;
            query = query.whereIn(self.pivot.related, keys.into_values().collect());
        }
        query.delete().await
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

    async fn synchronize(
        &self,
        parent: &P,
        records: impl Iterator<Item = (Value, Attributes)>,
        detach: bool,
    ) -> DatabaseResult<SyncChanges> {
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
        self.keys
            .connection(parent)?
            .transaction(
                move |transaction| {
                    Box::pin(sync_pivot(
                        transaction,
                        pivot,
                        owner.clone(),
                        desired.clone(),
                        detach,
                    ))
                },
                3,
            )
            .await
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
            let parent_column = format!("{}.{}", self.pivot.table, self.pivot.foreign);
            let pivot_key = format!("{}.{}", self.pivot.table, self.pivot.related);
            let related_key = format!("{}.{}", R::metadata().table, self.keys.related);
            let related_columns = format!("{}.*", R::metadata().table);
            let pivot_names: Vec<_> = [self.pivot.foreign, self.pivot.related]
                .into_iter()
                .chain(self.pivot.morph.map(|(kind, _)| kind))
                .chain(self.pivot_columns.iter().copied())
                .collect();
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
                let query = query
                    .scoped()?
                    .0
                    .join(self.pivot.table, &related_key, "=", &pivot_key);
                let rows = self
                    .pivot
                    .restrict_type(query, true)
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
                        let mut state =
                            ModelState::hydrated_attributes(model_attributes, connection.clone());
                        let attributes = pivot_names
                            .iter()
                            .zip(pivot)
                            .map(|(name, (_, value))| ((*name).into(), value))
                            .collect();
                        state
                            .relations
                            .insert(self.pivot_accessor.into(), Pivot::new(attributes));
                        owners.push(owner);
                        models.push(R::from_state(state));
                    }
                }
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

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(self.match_many(parents))
    }
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

type DesiredPivots = IndexMap<Vec<u8>, (Value, Attributes)>;

async fn sync_pivot(
    transaction: &mut Transaction,
    pivot: PivotKeys,
    owner: Value,
    desired: Arc<DesiredPivots>,
    detach: bool,
) -> DatabaseResult<SyncChanges> {
    let query = transaction.table(pivot.table).select([pivot.related]);
    let current = pivot.for_owner(query, &owner).lockForUpdate().get().await?;
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
        pivot
            .for_owner(query, &owner)
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
            row.extend(attributes.clone());
            inserts
                .entry(row.keys().cloned().collect())
                .or_default()
                .push(row);
            changes.attached.push(key.clone());
        } else if !attributes.is_empty() {
            let query = transaction.table(pivot.table);
            let affected = pivot
                .for_owner(query, &owner)
                .r#where(pivot.related, "=", key.clone())
                .whereNot(|conditions| {
                    attributes
                        .iter()
                        .fold(conditions, |conditions, (column, value)| {
                            conditions.whereNullSafeEquals(column, value.clone())
                        })
                })
                .update(attributes.clone())
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
        let query = self.pivot.restrict_type(query, true);
        Ok(match target_keys {
            Some(keys) => query.whereIn(&target_key, keys),
            None => query,
        })
    }
}
