//! 从模型写连接回读已有记录的原始属性。

use super::{
    SaveContext, SaveModel, attributes_from_row, model_events::fire_model_event_in_context,
};
use crate::{
    Db, Error, Result,
    database::{
        DatabaseTransaction, DbBackend, QueryBuilder, TransactionContext, sea_orm::QueryResult,
    },
};
use serde_json::{Map, Value};

/// 为已有数据库记录构造独立模型实例。
pub trait HydrateModel: SaveModel + Sized {
    /// 使用当前模型的连接声明构造新实例，将原始行同时作为当前属性和基线。
    ///
    /// 新实例应标记为已存在，且不标记为刚创建。该步骤不运行 setter 或模型事件。
    fn hydrate(&self, raw: Map<String, Value>) -> Result<Self>;
}

/// 按原主键从写连接水合独立模型，触发该模型的 retrieved 事件。
///
/// 新模型或查无记录时返回 None。触发事件时原实例保持原状。
/// 关系预加载仍由后续关系契约接入。
pub async fn fresh<M: HydrateModel>(model: &M, context: &SaveContext<'_>) -> Result<Option<M>> {
    if !model.exists() {
        return Ok(None);
    }
    let db = model
        .metadata()
        .resolve_connection(context.databases, None)?;
    let query = refresh_query(model, &db)?;
    let Some(row) = db.with_primary(query.first()).await?? else {
        return Ok(None);
    };
    Ok(Some(hydrate_retrieved(model, context, row, None).await?))
}

/// 以原主键从模型的写连接回读，原子替换当前属性及原始基线。
///
/// 新模型直接返回。记录不存在时返回 404，保留已有内存状态；回读不执行 setter。
/// 先在独立模型上派发 retrieved，再安装该模型的当前原始属性。已载入关系的重载仍待接入。
pub async fn refresh<'a, M: HydrateModel>(
    model: &'a mut M,
    context: &SaveContext<'_>,
) -> Result<&'a mut M> {
    if !model.exists() {
        return Ok(model);
    }
    let db = model
        .metadata()
        .resolve_connection(context.databases, None)?;
    let query = refresh_query(model, &db)?;
    let row = db.with_primary(query.first()).await??;
    install_row(model, context, row, None).await
}

/// 在调用者的写事务中锁定并回读已有模型。
///
/// 事务必须由模型所选连接签发。MySQL/PostgreSQL 使用行锁，SQLite 使用该事务
/// 已取得的写锁。回读结束后锁仍归调用者的事务持有，映射的 retrieved 事件可借用该事务。
pub async fn refresh_for_update<'a, M: HydrateModel>(
    model: &'a mut M,
    context: &SaveContext<'_>,
    transaction: &TransactionContext<'_>,
) -> Result<&'a mut M> {
    if !model.exists() {
        return Ok(model);
    }
    let db = model
        .metadata()
        .resolve_connection(context.databases, None)?;
    if !transaction.is_from(&db) {
        return Err(Error::custom(
            "model_refresh_transaction_source_mismatch",
            "锁定回读事务不属于模型连接",
        ));
    }
    let query = refresh_query(model, &db)?.using_transaction(transaction.transaction());
    let query = if db.backend()? == DbBackend::Sqlite {
        query
    } else {
        query.lock_for_update()?
    };
    let row = query.first().await?;
    install_row(model, context, row, Some(transaction.transaction())).await
}

fn refresh_query<'tx, M: SaveModel>(model: &M, db: &Db) -> Result<QueryBuilder<'tx, 'tx>> {
    let metadata = model.metadata();
    let key = model.attributes().key_for_save_query(metadata)?;
    let key = model.bind_attribute(&metadata.key_name, &key)?;
    db.table(&metadata.table)?.where_eq(&metadata.key_name, key)
}

async fn hydrate_retrieved<M: HydrateModel>(
    model: &M,
    context: &SaveContext<'_>,
    row: QueryResult,
    transaction: Option<&DatabaseTransaction>,
) -> Result<M> {
    let retrieved = model.hydrate(attributes_from_row(&row)?)?;
    fire_model_event_in_context(
        context.events,
        retrieved.metadata(),
        &retrieved,
        "retrieved",
        false,
        retrieved.mapped_event("retrieved"),
        transaction,
    )
    .await?;
    Ok(retrieved)
}

async fn install_row<'a, M: HydrateModel>(
    model: &'a mut M,
    context: &SaveContext<'_>,
    row: Option<QueryResult>,
    transaction: Option<&DatabaseTransaction>,
) -> Result<&'a mut M> {
    let row = row.ok_or_else(|| {
        Error::custom_with_status(http::StatusCode::NOT_FOUND, "model_not_found", "记录不存在")
    })?;
    let retrieved = hydrate_retrieved(model, context, row, transaction).await?;
    let attributes = retrieved.attributes().raw_attributes()?;
    model
        .attributes_mut()
        .install_refreshed_attributes(attributes)?;
    Ok(model)
}
