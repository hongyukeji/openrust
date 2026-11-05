//! 既有 `SharedLocks` 的单机 SQLite 方言，不是 Laravel `DatabaseLock` 的独立替代体系。
//!
//! SQLite 3.26 已支持条件 UPSERT；首先写入取得数据库写锁，再在同一事务读回。
//! 事务内持有者校验使用条件无变化 UPDATE，不能通过删掉 `FOR UPDATE` 改为无锁读取。

use super::{Db, DbBackend, Error, LockAcquire, LockLease, Result};
use sea_orm::{ConnectionTrait, DatabaseTransaction, Statement};

// SQLite 的 now 在同一 sqlite3_step 中稳定；3.26 不使用较新的 subsec/unixepoch。
const NOW_MS: &str = "(CAST(strftime('%s','now') AS INTEGER) * 1000 + CAST(substr(strftime('%f','now'),4,3) AS INTEGER))";

fn ensure_sqlite(backend: DbBackend) -> Result<()> {
    if backend == DbBackend::Sqlite {
        Ok(())
    } else {
        Err(super::unsupported_backend())
    }
}

/// 获取或接管租约；调用方已校验 key、生成 owner token 并转换受限 TTL。
pub(super) async fn acquire(
    db: &Db,
    key_hash: String,
    owner_token: String,
    ttl_ms: i64,
) -> Result<LockAcquire> {
    ensure_sqlite(db.backend()?)?;
    let table = db.quoted_table_name("shared_locks")?;
    let sql = format!(
        "INSERT INTO {table} (key_hash,owner_token,expires_at_ms,fencing_token) VALUES (?,?,{NOW_MS}+?,1) ON CONFLICT(key_hash) DO UPDATE SET owner_token=excluded.owner_token,expires_at_ms=excluded.expires_at_ms,fencing_token={table}.fencing_token+1 WHERE {table}.expires_at_ms<={NOW_MS}"
    );
    // 保留唯一 Db 事务事件/回滚生命周期。首条 UPSERT 是写操作，不先建立读快照。
    db.transaction(move |transaction| {
        Box::pin(async move {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    [
                        key_hash.clone().into(),
                        owner_token.clone().into(),
                        ttl_ms.into(),
                    ],
                ))
                .await
                .map_err(Error::internal)?;
            let row = transaction.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("SELECT owner_token,fencing_token,expires_at_ms FROM {table} WHERE key_hash=?"),
            [key_hash.clone().into()])).await.map_err(Error::internal)?
            .ok_or_else(|| Error::internal(std::io::Error::other("shared lock row disappeared")))?;
            let stored_owner: String = row.try_get("", "owner_token").map_err(Error::internal)?;
            let fencing_token: i64 = row.try_get("", "fencing_token").map_err(Error::internal)?;
            let expires_at_ms: i64 = row.try_get("", "expires_at_ms").map_err(Error::internal)?;
            Ok(if stored_owner == owner_token {
                LockAcquire::Acquired(LockLease {
                    key_hash,
                    owner_token,
                    fencing_token,
                    expires_at_ms,
                })
            } else {
                LockAcquire::Busy { expires_at_ms }
            })
        })
    })
    .await
}

/// 未过期且仍由同一 owner/fence 持有时续约；单条条件写入不创建额外事务。
pub(super) async fn renew(db: &Db, lease: &LockLease, ttl_ms: i64) -> Result<bool> {
    ensure_sqlite(db.backend()?)?;
    let table = db.quoted_table_name("shared_locks")?;
    let result = db.primary_connection()?.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
        format!("UPDATE {table} SET expires_at_ms={NOW_MS}+? WHERE key_hash=? AND owner_token=? AND fencing_token=? AND expires_at_ms>{NOW_MS}"),
        [ttl_ms.into(), lease.key_hash.clone().into(), lease.owner_token.clone().into(), lease.fencing_token.into()]))
        .await.map_err(Error::internal)?;
    Ok(result.rows_affected() == 1)
}

/// 在调用方的原事务内取得并保留写锁；过期或旧 owner 明确拒绝，不重开事务。
pub(super) async fn assert_current_in_transaction(
    db: &Db,
    transaction: &DatabaseTransaction,
    lease: &LockLease,
) -> Result<()> {
    ensure_sqlite(db.backend()?)?;
    ensure_sqlite(transaction.get_database_backend())?;
    let table = db.quoted_table_name("shared_locks")?;
    let result = transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
        format!("UPDATE {table} SET fencing_token=fencing_token WHERE key_hash=? AND owner_token=? AND fencing_token=? AND expires_at_ms>{NOW_MS}"),
        [lease.key_hash.clone().into(), lease.owner_token.clone().into(), lease.fencing_token.into()]))
        .await.map_err(Error::internal)?;
    if result.rows_affected() == 1 {
        Ok(())
    } else {
        Err(Error::custom(
            "shared_lock_lost",
            "shared lock lease expired or was acquired by another owner",
        ))
    }
}

#[cfg(test)]
#[path = "sqlite_locks_tests.rs"]
mod tests;
