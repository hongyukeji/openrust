//! 多对多关系对当前父模型中间表行的写入。

use super::super::{
    MassAssignment,
    save::{cancelled, notify, save_scoped_pivot},
    timestamps::fresh_timestamp,
};
use super::{
    BelongsToMany, CustomPivot, HydrateModel, ModelAttributes, PivotSource, SaveContext,
    relation_key,
};
use crate::{
    Db, Error, Result,
    database::{
        QueryBuilder, TransactionContext,
        eloquent::{SaveModel, attributes_from_row, save_with_query},
        sea_orm::Value as SqlValue,
    },
    support::BoxFuture,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// 同步时希望保留的关联键及中间表属性。
pub struct PivotAttachment<'a> {
    /// 关联模型键。
    pub related_key: Value,
    /// 新增或更新时写入的中间表属性。空集合保留已有属性。
    pub attributes: Vec<(&'a str, SqlValue)>,
}

/// 同步实际新增、移除及更新的关联键。
#[derive(Default)]
pub struct PivotChanges {
    /// 新增的关联键。
    pub attached: Vec<Value>,
    /// 移除的关联键。
    pub detached: Vec<Value>,
    /// 数据库报告已更新的关联键。
    pub updated: Vec<Value>,
}

/// 切换关系时实际新增或移除的关联键。
#[derive(Default)]
pub struct PivotToggleChanges {
    /// 原先未关联，现已新增的键。
    pub attached: Vec<Value>,
    /// 原先已关联，现已移除的键。
    pub detached: Vec<Value>,
}

pub(super) type PivotInserter = for<'q> fn(
    Vec<(&str, SqlValue)>,
    PivotSource<'_>,
    SaveContext<'q>,
    QueryBuilder<'q, 'q>,
) -> BoxFuture<'q, Result<u64>>;

pub(super) fn insert_custom_pivot<'q, P: CustomPivot>(
    attributes: Vec<(&str, SqlValue)>,
    source: PivotSource<'_>,
    context: SaveContext<'q>,
    query: QueryBuilder<'q, 'q>,
) -> BoxFuture<'q, Result<u64>> {
    let model = attributes
        .into_iter()
        .map(|(name, value)| Ok((name.to_owned(), P::raw_pivot_attribute(name, value)?)))
        .collect::<Result<_>>()
        .and_then(|attributes| P::from_raw_pivot(ModelAttributes::new(attributes), source, false));
    Box::pin(async move {
        let mut model = model?;
        Ok(u64::from(
            save_with_query(&mut model, &context, query).await?,
        ))
    })
}

pub(super) type PivotUpdater = for<'q> fn(
    Map<String, Value>,
    Vec<(&str, SqlValue)>,
    PivotSource<'_>,
    SaveContext<'q>,
    QueryBuilder<'q, 'q>,
) -> BoxFuture<'q, Result<u64>>;

pub(super) fn update_custom_pivot<'q, P: CustomPivot>(
    raw: Map<String, Value>,
    attributes: Vec<(&str, SqlValue)>,
    source: PivotSource<'_>,
    context: SaveContext<'q>,
    query: QueryBuilder<'q, 'q>,
) -> BoxFuture<'q, Result<u64>> {
    let model = P::from_raw_pivot(ModelAttributes::new(raw), source, true);
    let input = attributes
        .into_iter()
        .map(|(name, value)| Ok((name.to_owned(), P::raw_pivot_attribute(name, value)?)))
        .collect::<Result<_>>();
    Box::pin(async move {
        let mut model = model?;
        if !model.exists() {
            return Err(Error::custom(
                "model_custom_pivot_not_persisted",
                "自定义中间表模型未保留已有记录状态",
            ));
        }
        let metadata = model.metadata();
        model
            .attributes()
            .fill(metadata, input?, &MassAssignment::new(context.databases))
            .await?;
        if !model.attributes().is_dirty(metadata, &[])? {
            return Ok(0);
        }
        let query = constrain_pivot_identity(&model, query)?;
        save_scoped_pivot(&mut model, &context, query).await?;
        Ok(1)
    })
}

pub(super) type PivotDeleter = for<'q> fn(
    Map<String, Value>,
    PivotSource<'_>,
    &'q SaveContext<'_>,
    QueryBuilder<'q, 'q>,
) -> BoxFuture<'q, Result<u64>>;

pub(super) fn delete_custom_pivot<'q, P: CustomPivot>(
    raw: Map<String, Value>,
    source: PivotSource<'_>,
    context: &'q SaveContext<'_>,
    query: QueryBuilder<'q, 'q>,
) -> BoxFuture<'q, Result<u64>> {
    let model = P::from_raw_pivot(ModelAttributes::new(raw), source, true);
    Box::pin(async move {
        let mut model = model?;
        if !model.exists() {
            return Err(Error::custom(
                "model_custom_pivot_not_persisted",
                "自定义中间表模型未保留已有记录状态",
            ));
        }
        let transaction = query.transaction_ref()?;
        let metadata = model.metadata().clone();
        if cancelled(&model, &metadata, context.events, "deleting", transaction).await? {
            return Ok(0);
        }
        if context.touch {
            model.touch_owners(transaction).await?;
        }
        let affected = constrain_pivot_identity(&model, query)?.delete().await?;
        model.mark_pivot_deleted();
        notify(&model, &metadata, context.events, "deleted", transaction).await?;
        Ok(affected)
    })
}

fn constrain_pivot_identity<'q, M: SaveModel>(
    model: &M,
    query: QueryBuilder<'q, 'q>,
) -> Result<QueryBuilder<'q, 'q>> {
    let key_name = &model.metadata().key_name;
    match model.attributes().get_raw_original(key_name)? {
        Some(key) if !key.is_null() => {
            query.where_eq(key_name, model.bind_attribute(key_name, &key)?)
        }
        _ => Ok(query),
    }
}

impl<R: HydrateModel> BelongsToMany<'_, R> {
    /// 新增一条中间表关系。额外属性不得覆盖任一关系键。
    pub async fn attach<'v, M: SaveModel>(
        &'v self,
        parent: &mut M,
        related_key: &Value,
        attributes: Vec<(&'v str, SqlValue)>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<u64> {
        self.validate_touch_transaction(parent, context, transaction)?;
        let affected = self
            .insert_pivot(parent, related_key, attributes, context, transaction)
            .await?;
        self.touch_after_write(parent, context, transaction).await?;
        Ok(affected)
    }

    async fn insert_pivot<'v, M: SaveModel>(
        &'v self,
        parent: &M,
        related_key: &Value,
        mut attributes: Vec<(&'v str, SqlValue)>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<u64> {
        self.validate_pivot_attributes(&attributes)?;
        for filter in self
            .pivot_equalities
            .iter()
            .rev()
            .filter(|filter| filter.default_on_attach)
        {
            if !attributes
                .iter()
                .any(|(column, _)| column.eq_ignore_ascii_case(filter.column))
            {
                attributes.push((filter.column, filter.value.clone()));
            }
        }
        self.apply_pivot_timestamps(&mut attributes, parent, context, false)?;
        let metadata = parent.metadata();
        let db = self.pivot_mutation_connection(parent, context)?;
        let query = self.pivot_write_query(&db, transaction)?;
        let parent_key = self.bind_parent_key(parent)?;
        attributes.push((self.keys.parent_foreign_key, parent_key));
        attributes.push((
            self.keys.related_foreign_key,
            self.bind_related_key(related_key)?,
        ));
        if let Some(custom) = self.custom_pivot {
            let source = PivotSource {
                keys: &self.keys,
                parent: metadata,
                timestamps: self.timestamps,
            };
            return (custom.insert)(attributes, source, SaveContext { ..*context }, query).await;
        }
        query.insert(attributes).await
    }

    /// 更新该父模型和关联键对应的中间表属性，返回数据库报告的影响行数。
    pub async fn update_existing_pivot<'v, M: SaveModel>(
        &'v self,
        parent: &mut M,
        related_key: &Value,
        attributes: Vec<(&'v str, SqlValue)>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<u64> {
        self.validate_touch_transaction(parent, context, transaction)?;
        let custom = self.custom_pivot.is_some();
        let has_input = !attributes.is_empty();
        let affected = self
            .update_pivot(parent, related_key, attributes, context, transaction)
            .await?;
        if custom || has_input || self.timestamps.is_some() {
            self.touch_after_write(parent, context, transaction).await?;
        }
        Ok(affected)
    }

    async fn update_pivot<'v, M: SaveModel>(
        &'v self,
        parent: &M,
        related_key: &Value,
        mut attributes: Vec<(&'v str, SqlValue)>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<u64> {
        self.validate_pivot_attributes(&attributes)?;
        if let Some(custom) = self.custom_pivot {
            let metadata = parent.metadata();
            let db = metadata.resolve_connection(context.databases, None)?;
            let query = self
                .constrain_pivot(self.pivot_write_query(&db, transaction)?)?
                .where_eq(self.keys.parent_foreign_key, self.bind_parent_key(parent)?)?
                .where_eq(
                    self.keys.related_foreign_key,
                    self.bind_related_key(related_key)?,
                )?;
            let Some(row) = db.with_primary(query.first()).await?? else {
                return Ok(0);
            };
            let source = PivotSource {
                keys: &self.keys,
                parent: metadata,
                timestamps: self.timestamps,
            };
            return (custom.update)(
                attributes_from_row(&row)?,
                attributes,
                source,
                SaveContext { ..*context },
                query,
            )
            .await;
        }
        self.apply_pivot_timestamps(&mut attributes, parent, context, true)?;
        if attributes.is_empty() {
            return Ok(0);
        }
        let (query, parent_key) = self.pivot_for_write(parent, context, transaction)?;
        query
            .where_eq(self.keys.parent_foreign_key, parent_key)?
            .where_eq(
                self.keys.related_foreign_key,
                self.bind_related_key(related_key)?,
            )?
            .update(attributes)
            .await
    }

    /// 删除该父模型的指定中间表关系。None 删除该父模型的全部关系，空切片直接返回。
    pub async fn detach<M: SaveModel>(
        &self,
        parent: &mut M,
        related_keys: Option<&[Value]>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<u64> {
        if related_keys.is_some_and(<[Value]>::is_empty) {
            return Ok(0);
        }
        self.validate_touch_transaction(parent, context, transaction)?;
        let affected = self
            .delete_pivots(parent, related_keys, context, transaction)
            .await?;
        self.touch_after_write(parent, context, transaction).await?;
        Ok(affected)
    }

    async fn delete_pivots<M: SaveModel>(
        &self,
        parent: &M,
        related_keys: Option<&[Value]>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<u64> {
        if related_keys.is_some_and(<[Value]>::is_empty) {
            return Ok(0);
        }
        let metadata = parent.metadata();
        let db = self.pivot_mutation_connection(parent, context)?;
        let parent_query = self
            .constrain_pivot(self.pivot_write_query(&db, transaction)?)?
            .where_eq(self.keys.parent_foreign_key, self.bind_parent_key(parent)?)?;
        let query = match related_keys {
            Some(keys) => parent_query.clone().where_in(
                self.keys.related_foreign_key,
                keys.iter()
                    .map(|key| self.bind_related_key(key))
                    .collect::<Result<Vec<_>>>()?,
            )?,
            None => parent_query.clone(),
        };
        let Some(custom) = self.custom_pivot else {
            return query.delete().await;
        };
        let mut affected = 0;
        for row in db.with_primary(query.get()).await?? {
            let raw = attributes_from_row(&row)?;
            let key = raw.get(self.keys.related_foreign_key).ok_or_else(|| {
                Error::custom("model_pivot_related_key_missing", "中间表查询缺少关联键")
            })?;
            let query = parent_query
                .clone()
                .where_eq(self.keys.related_foreign_key, self.bind_related_key(key)?)?;
            let source = PivotSource {
                keys: &self.keys,
                parent: metadata,
                timestamps: self.timestamps,
            };
            affected += (custom.delete)(raw, source, context, query).await?;
        }
        Ok(affected)
    }

    /// 在调用者的写事务中同步中间表。调用者应在错误时回滚该事务。
    ///
    /// `detaching` 为 false 时保留输入未列出的关联。重复输入按首次位置处理，
    /// 使用同一关联键最后提供的属性。空属性不更新已经存在的关联。
    pub async fn sync<'v, M: SaveModel>(
        &'v self,
        parent: &mut M,
        attachments: Vec<PivotAttachment<'v>>,
        detaching: bool,
        context: &SaveContext<'_>,
        transaction: &TransactionContext<'_>,
    ) -> Result<PivotChanges> {
        let entries = self.prepare_attachments(attachments)?;
        let requested: BTreeSet<_> = entries.iter().map(|(key, _)| key.as_str()).collect();
        let snapshot = self
            .current_pivot_keys(parent, context, transaction)
            .await?;
        let current: BTreeSet<_> = snapshot.iter().map(|(key, _)| key.as_str()).collect();
        let detached: Vec<_> = snapshot
            .iter()
            .filter(|(key, _)| detaching && !requested.contains(key.as_str()))
            .map(|(_, related)| related.clone())
            .collect();
        self.validate_touch_transaction(parent, context, Some(transaction))?;
        self.delete_pivots(parent, Some(&detached), context, Some(transaction))
            .await?;
        let mut changes = PivotChanges {
            detached,
            ..Default::default()
        };
        for (key, entry) in entries {
            if current.contains(key.as_str()) {
                if entry.attributes.is_empty() {
                    continue;
                }
                let affected = self
                    .update_pivot(
                        parent,
                        &entry.related_key,
                        entry.attributes,
                        context,
                        Some(transaction),
                    )
                    .await?;
                if affected > 0 {
                    changes.updated.push(entry.related_key);
                }
            } else {
                self.insert_pivot(
                    parent,
                    &entry.related_key,
                    entry.attributes,
                    context,
                    Some(transaction),
                )
                .await?;
                changes.attached.push(entry.related_key);
            }
        }
        if !changes.attached.is_empty()
            || !changes.detached.is_empty()
            || !changes.updated.is_empty()
        {
            self.touch_after_write(parent, context, Some(transaction))
                .await?;
        }
        Ok(changes)
    }

    /// 在调用者的写事务中移除已关联的输入键，并新增尚未关联的输入键。
    ///
    /// 只在新增时写入输入属性。调用者应在错误时回滚该事务。
    pub async fn toggle<'v, M: SaveModel>(
        &'v self,
        parent: &mut M,
        attachments: Vec<PivotAttachment<'v>>,
        context: &SaveContext<'_>,
        transaction: &TransactionContext<'_>,
    ) -> Result<PivotToggleChanges> {
        let entries = self.prepare_attachments(attachments)?;
        let requested: BTreeSet<_> = entries.iter().map(|(key, _)| key.as_str()).collect();
        let snapshot = self
            .current_pivot_keys(parent, context, transaction)
            .await?;
        let current: BTreeSet<_> = snapshot.iter().map(|(key, _)| key.as_str()).collect();
        let detached: Vec<_> = snapshot
            .iter()
            .filter(|(key, _)| requested.contains(key.as_str()))
            .map(|(_, related)| related.clone())
            .collect();
        self.validate_touch_transaction(parent, context, Some(transaction))?;
        self.delete_pivots(parent, Some(&detached), context, Some(transaction))
            .await?;
        let mut changes = PivotToggleChanges {
            detached,
            ..Default::default()
        };
        for (key, entry) in entries {
            if !current.contains(key.as_str()) {
                self.insert_pivot(
                    parent,
                    &entry.related_key,
                    entry.attributes,
                    context,
                    Some(transaction),
                )
                .await?;
                changes.attached.push(entry.related_key);
            }
        }
        if !changes.attached.is_empty() || !changes.detached.is_empty() {
            self.touch_after_write(parent, context, Some(transaction))
                .await?;
        }
        Ok(changes)
    }

    fn prepare_attachments<'v>(
        &self,
        attachments: Vec<PivotAttachment<'v>>,
    ) -> Result<Vec<(String, PivotAttachment<'v>)>> {
        let mut requested = BTreeMap::<String, (usize, PivotAttachment<'v>)>::new();
        for (index, entry) in attachments.into_iter().enumerate() {
            self.validate_pivot_attributes(&entry.attributes)?;
            let key = relation_key(Some(&entry.related_key))?.ok_or_else(|| {
                Error::custom(
                    "model_pivot_related_key_missing",
                    "中间表写入需要有效关联键",
                )
            })?;
            self.related
                .bind_attribute(self.keys.related_key, &entry.related_key)?;
            let order = requested.get(&key).map_or(index, |(order, _)| *order);
            requested.insert(key, (order, entry));
        }
        let mut entries: Vec<_> = requested.into_iter().collect();
        entries.sort_by_key(|(_, (position, _))| *position);
        Ok(entries
            .into_iter()
            .map(|(key, (_, entry))| (key, entry))
            .collect())
    }

    async fn current_pivot_keys<M: SaveModel>(
        &self,
        parent: &M,
        context: &SaveContext<'_>,
        transaction: &TransactionContext<'_>,
    ) -> Result<Vec<(String, Value)>> {
        let db = self.pivot_mutation_connection(parent, context)?;
        let query = self.constrain_pivot(self.pivot_write_query(&db, Some(transaction))?)?;
        let parent_key = self.bind_parent_key(parent)?;
        let rows = query
            .where_eq(self.keys.parent_foreign_key, parent_key)?
            .select(&[self.keys.related_foreign_key])?
            .get()
            .await?;
        rows.into_iter()
            .map(|row| {
                let mut attributes = attributes_from_row(&row)?;
                let related = attributes
                    .remove(self.keys.related_foreign_key)
                    .ok_or_else(|| {
                        Error::custom("model_pivot_related_key_missing", "中间表查询缺少关联键")
                    })?;
                let key = relation_key(Some(&related))?.ok_or_else(|| {
                    Error::custom("model_pivot_related_key_missing", "中间表查询返回空关联键")
                })?;
                Ok((key, related))
            })
            .collect()
    }

    /// 使用关联模型的更新时间列，批量更新时间戳；不派发逐模型保存事件。
    ///
    /// 更新范围为当前仍关联的键，遵循模型的时间戳禁用范围。
    pub async fn touch_related<M: SaveModel>(
        &self,
        parent: &M,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<u64> {
        let metadata = self.related.metadata();
        let Some(column) = metadata
            .updated_at_column
            .as_deref()
            .filter(|_| metadata.uses_timestamps())
        else {
            return Ok(0);
        };
        let db = metadata
            .resolve_connection(context.databases, parent.metadata().connection.as_deref())?;
        let (pivot, parent_key) = self.pivot_for_write(parent, context, transaction)?;
        let pivot = pivot
            .where_eq(self.keys.parent_foreign_key, parent_key)?
            .select(&[self.keys.related_foreign_key])?;
        let rows = db.with_primary(pivot.get()).await??;
        let mut keys = Vec::with_capacity(rows.len());
        for row in rows {
            let attributes = attributes_from_row(&row)?;
            let key = attributes
                .get(self.keys.related_foreign_key)
                .ok_or_else(|| {
                    Error::custom("model_pivot_related_key_missing", "中间表查询缺少关联键")
                })?;
            keys.push(self.bind_related_key(key)?);
        }
        if keys.is_empty() {
            return Ok(0);
        }

        let format = metadata.date_format.as_deref().unwrap_or("Y-m-d H:i:s");
        let calendar = context.calendar;
        let now = fresh_timestamp(calendar)?.format(format)?;
        let query = db
            .table(&metadata.table)?
            .where_in(self.keys.related_key, keys)?;
        let query = match transaction {
            Some(transaction) => query.using_transaction(transaction.transaction()),
            None => query,
        };
        query
            .update(vec![(
                column,
                self.related.bind_attribute(column, &Value::String(now))?,
            )])
            .await
    }

    fn validate_touch_transaction<M: SaveModel>(
        &self,
        parent: &M,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<()> {
        let Some(transaction) = transaction.filter(|_| context.touch) else {
            return Ok(());
        };
        let parent = parent.metadata();
        let related = self.related.metadata();
        if self.touch_parent_on_write
            && parent.uses_timestamps()
            && !transaction.is_from(&parent.resolve_connection(context.databases, None)?)
        {
            return Err(Error::custom(
                "model_relation_transaction_source_mismatch",
                "触碰父模型的事务不属于父模型连接",
            ));
        }
        if self.touch_related_on_write
            && related.uses_timestamps()
            && related.updated_at_column.is_some()
            && !transaction.is_from(
                &related.resolve_connection(context.databases, parent.connection.as_deref())?,
            )
        {
            return Err(Error::custom(
                "model_relation_transaction_source_mismatch",
                "触碰关联模型的事务不属于关联模型连接",
            ));
        }
        Ok(())
    }

    async fn touch_after_write<M: SaveModel>(
        &self,
        parent: &mut M,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'_>>,
    ) -> Result<()> {
        if !context.touch {
            return Ok(());
        }
        if self.touch_parent_on_write && parent.metadata().uses_timestamps() {
            let metadata = parent.metadata();
            let db = metadata.resolve_connection(context.databases, None)?;
            let query = db.table(&metadata.table)?;
            let query = match transaction {
                Some(transaction) if transaction.is_from(&db) => {
                    query.using_transaction(transaction.transaction())
                }
                Some(_) => {
                    return Err(Error::custom(
                        "model_relation_transaction_source_mismatch",
                        "触碰父模型的事务不属于父模型连接",
                    ));
                }
                None => query,
            };
            parent
                .attributes()
                .update_timestamps(metadata, parent.exists(), context.calendar)
                .await?;
            save_with_query(parent, context, query).await?;
        }
        if self.touch_related_on_write {
            self.touch_related(parent, context, transaction).await?;
        }
        Ok(())
    }

    fn apply_pivot_timestamps<'v, M: SaveModel>(
        &'v self,
        attributes: &mut Vec<(&'v str, SqlValue)>,
        parent: &M,
        context: &SaveContext<'_>,
        exists: bool,
    ) -> Result<()> {
        let Some((created_at, updated_at)) = self.timestamps else {
            return Ok(());
        };
        let format = parent
            .metadata()
            .date_format
            .as_deref()
            .unwrap_or("Y-m-d H:i:s");
        let calendar = context.calendar;
        let value = fresh_timestamp(calendar)?.format(format)?;
        if exists {
            attributes.retain(|(name, _)| !name.eq_ignore_ascii_case(updated_at));
            attributes.push((updated_at, value.into()));
        } else {
            for column in [created_at, updated_at] {
                if !attributes
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(column))
                {
                    attributes.push((column, value.clone().into()));
                }
            }
        }
        Ok(())
    }

    fn pivot_mutation_connection<M: SaveModel>(
        &self,
        parent: &M,
        context: &SaveContext<'_>,
    ) -> Result<Db> {
        let metadata = parent.metadata();
        if self.custom_pivot.is_some() {
            metadata.resolve_connection(context.databases, None)
        } else {
            self.related
                .metadata()
                .resolve_connection(context.databases, metadata.connection.as_deref())
        }
    }

    fn pivot_for_write<'q, M: SaveModel>(
        &self,
        parent: &M,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'q>>,
    ) -> Result<(QueryBuilder<'q, 'q>, SqlValue)> {
        let db = self
            .related
            .metadata()
            .resolve_connection(context.databases, parent.metadata().connection.as_deref())?;
        Ok((
            self.constrain_pivot(self.pivot_write_query(&db, transaction)?)?,
            self.bind_parent_key(parent)?,
        ))
    }

    fn constrain_pivot<'q>(&self, mut query: QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>> {
        for filter in &self.pivot_equalities {
            query = if filter.value == filter.value.as_null() {
                query.where_null(filter.column)?
            } else {
                query.where_eq(filter.column, filter.value.clone())?
            };
        }
        Ok(query)
    }

    fn pivot_write_query<'q>(
        &self,
        db: &Db,
        transaction: Option<&TransactionContext<'q>>,
    ) -> Result<QueryBuilder<'q, 'q>> {
        let query = db.table(self.keys.table)?;
        match transaction {
            Some(transaction) if transaction.is_from(db) => {
                Ok(query.using_transaction(transaction.transaction()))
            }
            Some(_) => Err(Error::custom(
                "model_relation_transaction_source_mismatch",
                "关系写入事务不属于实际写入连接",
            )),
            None => Ok(query),
        }
    }

    fn bind_parent_key<M: SaveModel>(&self, parent: &M) -> Result<SqlValue> {
        let key = parent
            .attributes()
            .get_attribute(parent.metadata(), self.keys.parent_key)?;
        let key = key.filter(|key| !key.is_null()).ok_or_else(|| {
            Error::custom("model_pivot_parent_key_missing", "中间表写入需要有效父键")
        })?;
        relation_key(Some(&key))?;
        parent.bind_attribute(self.keys.parent_key, &key)
    }

    fn bind_related_key(&self, key: &Value) -> Result<SqlValue> {
        relation_key(Some(key))?.ok_or_else(|| {
            Error::custom(
                "model_pivot_related_key_missing",
                "中间表写入需要有效关联键",
            )
        })?;
        self.related.bind_attribute(self.keys.related_key, key)
    }

    pub(super) fn validate_pivot_attributes(&self, attributes: &[(&str, SqlValue)]) -> Result<()> {
        if attributes.iter().any(|(name, _)| {
            name.eq_ignore_ascii_case(self.keys.parent_foreign_key)
                || name.eq_ignore_ascii_case(self.keys.related_foreign_key)
        }) {
            return Err(Error::custom(
                "model_pivot_key_override",
                "额外中间表属性不能覆盖关系键",
            ));
        }
        Ok(())
    }
}
