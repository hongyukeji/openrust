//! 模型的保存流程，沿用 Query Builder、属性状态和模型事件。

use super::model_events::fire_model_event_in_context;
use super::{
    MappedModelEvent, MassAssignment, ModelAttributes, ModelMetadata, unguarded,
    without_model_events,
};
use crate::{
    Databases, Error, Result,
    database::{DatabaseTransaction, QueryBuilder, sea_orm},
    events::Events,
    support::{BoxFuture, CalendarContext, DataValue},
};
use serde_json::{Map, Value};
use std::any::Any;

/// 通过同一 Eloquent 属性存储保存的模型。
pub trait SaveModel: Any + Send + Sync {
    /// 返回当前模型声明。
    fn metadata(&self) -> &ModelMetadata;
    /// 返回模型自身的属性；监听器可观察或修改同一份状态。
    fn attributes(&self) -> &ModelAttributes;
    /// 取得同一属性存储的独占借用，使实际写库与变化发布之间没有其他安全借用。
    fn attributes_mut(&mut self) -> &mut ModelAttributes;
    /// 当前实例是否已持久化。
    fn exists(&self) -> bool;
    /// 在插入及主键赋值成功后记录 exists 与 wasRecentlyCreated。
    fn mark_created(&mut self);
    /// 为已启用唯一 ID 的模型生成下一个 ID；默认基础生成器返回 null。
    fn new_unique_id(&self) -> Result<Option<String>> {
        Ok(None)
    }
    /// 将原始属性转成实际列的数据库绑定类型，包括带类型的 SQL null。
    fn bind_attribute(&self, name: &str, value: &Value) -> Result<sea_orm::Value>;
    /// 更新模型声明的关联所有者时间戳；默认模型没有需要 touch 的所有者。
    ///
    /// 调用发生在 saved 之后、同步原始基线之前。
    fn touch_owners<'a>(
        &'a self,
        _transaction: Option<&'a DatabaseTransaction>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    /// 返回对应生命周期的应用映射事件。默认仅派发具名事件。
    fn mapped_event(&self, _event: &str) -> Option<&MappedModelEvent<'_, Self>>
    where
        Self: Sized,
    {
        None
    }
}

/// 持久化使用的同一应用数据库、派发器与时钟。
pub struct SaveContext<'a> {
    /// 选择默认或模型声明的连接。
    pub databases: &'a Databases,
    /// 已解析的应用派发器。None 时跳过事件。
    pub events: Option<&'a Events>,
    /// 应用时钟与时区。
    pub calendar: &'a CalendarContext,
    /// 保存后是否更新关联所有者时间戳，对应默认启用的 touch 选项。
    pub touch: bool,
}

impl<'a> SaveContext<'a> {
    /// 使用应用服务创建保存上下文，默认启用关联时间戳维护。
    pub const fn new(
        databases: &'a Databases,
        events: Option<&'a Events>,
        calendar: &'a CalendarContext,
    ) -> Self {
        Self {
            databases,
            events,
            calendar,
            touch: true,
        }
    }
}

/// 保存模型；返回 false 仅表示前置模型事件取消。
///
/// 已是 clean 时仍派发 saving/saved。非递增且没有属性的新模型不执行 INSERT，
/// 也不标为已创建。更新成功才发布 changes。保存后可调用模型的关联 touch 回调。
pub async fn save<M: SaveModel>(model: &mut M, context: &SaveContext<'_>) -> Result<bool> {
    let metadata = model.metadata();
    let db = metadata.resolve_connection(context.databases, None)?;
    let query = db.table(&metadata.table)?;
    save_with_query(model, context, query).await
}

/// 使用调用方已限定表和权限的查询保存同一模型。
///
/// 用于应用持有的事务或额外写入约束。调用者负责选择对应模型的表和连接，
/// 并负责事务提交；模型事件在当前调用中立即派发。
pub async fn save_with_query<M: SaveModel>(
    model: &mut M,
    context: &SaveContext<'_>,
    query: QueryBuilder<'_, '_>,
) -> Result<bool> {
    save_using(model, context, query, true).await
}

// 调用方已用原始中间表键限制查询，保留相同的模型保存生命周期。
pub(super) async fn save_scoped_pivot<M: SaveModel>(
    model: &mut M,
    context: &SaveContext<'_>,
    query: QueryBuilder<'_, '_>,
) -> Result<bool> {
    save_using(model, context, query, false).await
}

async fn save_using<M: SaveModel>(
    model: &mut M,
    context: &SaveContext<'_>,
    query: QueryBuilder<'_, '_>,
    constrain_primary_key: bool,
) -> Result<bool> {
    let metadata = model.metadata().clone();
    let transaction = query.transaction_ref()?;
    if cancelled(model, &metadata, context.events, "saving", transaction).await? {
        return Ok(false);
    }
    let saved = if model.exists() {
        if model.attributes().is_dirty(&metadata, &[])? {
            persist_update(model, &metadata, context, query, constrain_primary_key).await?
        } else {
            true
        }
    } else {
        insert(model, &metadata, context, query).await?
    };
    if saved {
        notify(model, &metadata, context.events, "saved", transaction).await?;
        if model.attributes().is_dirty(&metadata, &[])? && context.touch {
            model.touch_owners(transaction).await?;
        }
        model.attributes_mut().sync_original()?;
    }
    Ok(saved)
}

/// 在模型连接的事务内执行同一保存流程。
///
/// 前置事件取消返回 false。异常沿数据库事务规则回滚；模型的内存修改和
/// 已执行的外部监听器副作用仍遵循普通保存语义。
pub async fn save_or_fail<M: SaveModel>(model: &mut M, context: &SaveContext<'_>) -> Result<bool> {
    let metadata = model.metadata();
    let db = metadata.resolve_connection(context.databases, None)?;
    let table = metadata.table.clone();
    let mut state = (model, context, table, db.clone());
    db.transaction_with_state(&mut state, |(model, context, table, db), transaction| {
        Box::pin(async move {
            let query = db.table(table)?.using_transaction(transaction);
            save_with_query(&mut **model, context, query).await
        })
    })
    .await
}

/// 保存模型时停止本次调用链的模型事件，仍使用相同时间戳与 SQL 流程。
pub async fn save_quietly<M: SaveModel>(model: &mut M, context: &SaveContext<'_>) -> Result<bool> {
    without_model_events(save(model, context)).await
}

/// 已有模型先执行普通批量赋值再保存；未持久化实例直接返回 false。
pub async fn update<M: SaveModel>(
    model: &mut M,
    input: Map<String, Value>,
    assignment: &MassAssignment<'_>,
    context: &SaveContext<'_>,
) -> Result<bool> {
    if !model.exists() {
        return Ok(false);
    }
    model
        .attributes()
        .fill(model.metadata(), input, assignment)
        .await?;
    save(model, context).await
}

/// 已有模型先普通赋值，再用模型连接的事务保存。
pub async fn update_or_fail<M: SaveModel>(
    model: &mut M,
    input: Map<String, Value>,
    assignment: &MassAssignment<'_>,
    context: &SaveContext<'_>,
) -> Result<bool> {
    if !model.exists() {
        return Ok(false);
    }
    model
        .attributes()
        .fill(model.metadata(), input, assignment)
        .await?;
    save_or_fail(model, context).await
}

/// 在当前调用链停止模型事件，并执行同一 update 流程。
pub async fn update_quietly<M: SaveModel>(
    model: &mut M,
    input: Map<String, Value>,
    assignment: &MassAssignment<'_>,
    context: &SaveContext<'_>,
) -> Result<bool> {
    without_model_events(update(model, input, assignment, context)).await
}

/// 用应用工厂构建模型并强制批量赋值，然后通过同一保存流程创建。
///
/// 若事件取消，仍返回该新模型；调用者可通过 exists 区分是否成功写入。
pub async fn force_create<M: SaveModel>(
    new_model: impl FnOnce() -> Result<M>,
    input: Map<String, Value>,
    assignment: &MassAssignment<'_>,
    context: &SaveContext<'_>,
) -> Result<M> {
    unguarded(async {
        let mut model = new_model()?;
        if model.exists() {
            return Err(Error::custom(
                "model_force_create_existing",
                "创建工厂返回了既有模型",
            ));
        }
        model
            .attributes()
            .fill(model.metadata(), input, assignment)
            .await?;
        save(&mut model, context).await?;
        Ok(model)
    })
    .await
}

async fn persist_update<M: SaveModel>(
    model: &mut M,
    metadata: &ModelMetadata,
    context: &SaveContext<'_>,
    mut query: QueryBuilder<'_, '_>,
    constrain_primary_key: bool,
) -> Result<bool> {
    let transaction = query.transaction_ref()?;
    if cancelled(model, metadata, context.events, "updating", transaction).await? {
        return Ok(false);
    }
    if metadata.uses_timestamps() {
        model
            .attributes_mut()
            .update_timestamps(metadata, true, context.calendar)
            .await?;
    }
    let changes = model.attributes().get_dirty(metadata)?;
    if !changes.is_empty() {
        if constrain_primary_key {
            let key = model.attributes().key_for_save_query(metadata)?;
            let key = model.bind_attribute(&metadata.key_name, &key)?;
            query = query.where_eq(&metadata.key_name, key)?;
        }
        let bindings = bind_values(model, &changes)?;
        let attributes = model.attributes_mut();
        query.update(bindings).await?;
        attributes.sync_changes(metadata)?;
        notify(model, metadata, context.events, "updated", transaction).await?;
    }
    Ok(true)
}

async fn insert<M: SaveModel>(
    model: &mut M,
    metadata: &ModelMetadata,
    context: &SaveContext<'_>,
    query: QueryBuilder<'_, '_>,
) -> Result<bool> {
    let transaction = query.transaction_ref()?;
    if metadata.uses_unique_ids {
        model
            .attributes()
            .set_unique_ids(metadata, || model.new_unique_id())
            .await?;
    }
    if cancelled(model, metadata, context.events, "creating", transaction).await? {
        return Ok(false);
    }
    if metadata.uses_timestamps() {
        model
            .attributes_mut()
            .update_timestamps(metadata, false, context.calendar)
            .await?;
    }
    let raw = model.attributes().raw_attributes()?;
    let bindings = bind_values(model, &raw)?;
    if metadata.incrementing {
        let attributes = model.attributes_mut();
        let key = query
            .insert_get_id(bindings, Some(&metadata.key_name))
            .await?;
        attributes
            .set_attribute(metadata, &metadata.key_name, inserted_key(key)?)
            .await?;
    } else if bindings.is_empty() {
        return Ok(true);
    } else {
        query.insert(bindings).await?;
    }
    model.mark_created();
    notify(model, metadata, context.events, "created", transaction).await?;
    Ok(true)
}

fn bind_values<'a, M: SaveModel>(
    model: &M,
    values: &'a Map<String, Value>,
) -> Result<Vec<(&'a str, sea_orm::Value)>> {
    values
        .iter()
        .map(|(name, value)| {
            model
                .bind_attribute(name, value)
                .map(|bound| (name.as_str(), bound))
        })
        .collect()
}

fn inserted_key(key: sea_orm::Value) -> Result<Value> {
    Ok(match key {
        sea_orm::Value::BigInt(value) => value.map_or(Value::Null, Value::from),
        sea_orm::Value::BigUnsigned(value) => value.map_or(Value::Null, Value::from),
        sea_orm::Value::Bool(value) => value.map_or(Value::Null, Value::Bool),
        sea_orm::Value::String(value) => value.map_or(Value::Null, Value::String),
        sea_orm::Value::Bytes(value) => value.map_or(Value::Null, |bytes| {
            Value::Array(bytes.into_iter().map(Value::from).collect())
        }),
        sea_orm::Value::Float(None) | sea_orm::Value::Double(None) => Value::Null,
        _ => {
            return Err(Error::custom(
                "model_insert_key_unsupported",
                "返回的主键不能映射为模型原始值",
            ));
        }
    })
}

pub(super) async fn cancelled<M: SaveModel>(
    model: &M,
    metadata: &ModelMetadata,
    events: Option<&Events>,
    event: &str,
    transaction: Option<&DatabaseTransaction>,
) -> Result<bool> {
    Ok(matches!(
        fire_model_event_in_context(
            events,
            metadata,
            model,
            event,
            true,
            model.mapped_event(event),
            transaction,
        )
        .await?,
        DataValue::Bool(false)
    ))
}

pub(super) async fn notify<M: SaveModel>(
    model: &M,
    metadata: &ModelMetadata,
    events: Option<&Events>,
    event: &str,
    transaction: Option<&DatabaseTransaction>,
) -> Result<()> {
    fire_model_event_in_context(
        events,
        metadata,
        model,
        event,
        false,
        model.mapped_event(event),
        transaction,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
