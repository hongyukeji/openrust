//! 显式增强 Session driver 的 SQLite 精确截止与写入所有权。
//!
//! 沿用统一 `SessionStore` 的条件写入与绝对期限。
//! BEGIN IMMEDIATE 仅取得数据库写入所有权，不宣称 SQLite 提供行级锁。

use super::sqlite_deadline::to_offset;
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{Row, Sqlite, SqliteConnection, SqlitePool, pool::PoolConnection};
use time::OffsetDateTime;

use super::{SessionId, SessionRecord, SessionWriteMode, SessionWriteOutcome};
use super::{digest_id, invalid_record, limit_error, unavailable};
use crate::Result;

const GC_BATCH: i64 = 512;

// close_on_drop 是单向标志；仅在 COMMIT/ROLLBACK 明确成功后通过共享的
// S2 物理 collation 析构见证归还，确认本次使用的内存 handle 仍驻留，不从池上限或共享参数推断有其他 anchor。
// 未确认路径和取消只允许物理关闭；不把异步关闭排队描述为已经完成回滚。
struct OwnedSqliteTransaction {
    connection: PoolConnection<Sqlite>,
    pool: SqlitePool,
    require_residency: bool,
}

impl std::ops::Deref for OwnedSqliteTransaction {
    type Target = SqliteConnection;
    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}
impl std::ops::DerefMut for OwnedSqliteTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

impl OwnedSqliteTransaction {
    async fn begin(pool: &SqlitePool) -> Result<Self> {
        let mut connection = pool.acquire().await.map_err(|_| unavailable())?;
        // 必须在第一个可能改变事务状态的 await 之前设定，future 取消也不会回池。
        connection.close_on_drop();
        let require_residency = match memory_residency_required(&mut connection).await {
            Ok(required) => required,
            Err(error) => {
                connection.close().await.map_err(|_| unavailable())?;
                return Err(error);
            }
        };
        if sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .is_err()
        {
            connection.close().await.map_err(|_| unavailable())?;
            return Err(unavailable());
        }
        Ok(Self {
            connection,
            pool: pool.clone(),
            require_residency,
        })
    }

    async fn return_confirmed(self) -> Result<()> {
        let resident = crate::database::migration::return_sqlite_connection(self.connection).await;
        if self.require_residency && !matches!(resident, Ok(true)) {
            // 提交可能已经成功；确认失败只报告不可用，绝不谎报已回滚。
            // 保守关闭本池以免重建未知最后 handle，不宣称其他共享池的 handle 均已丢失。
            drop(self.pool.close());
            return Err(unavailable());
        }
        // 文件库正常 max_lifetime/after_release 回收不丢持久数据，false 允许成功。
        resident.map(|_| ()).map_err(|_| unavailable())
    }
}

async fn finish<T>(mut transaction: OwnedSqliteTransaction, outcome: Result<T>) -> Result<T> {
    match outcome {
        Ok(value) => {
            if sqlx::query("COMMIT")
                .execute(&mut *transaction)
                .await
                .is_err()
            {
                // COMMIT 错误后即使回滚成功也不复用此物理连接；两步均等待再报告失败。
                let rollback = sqlx::query("ROLLBACK").execute(&mut *transaction).await;
                let closed = transaction.connection.close().await;
                rollback.map_err(|_| unavailable())?;
                closed.map_err(|_| unavailable())?;
                return Err(unavailable());
            }
            transaction.return_confirmed().await?;
            Ok(value)
        }
        Err(error) => {
            if sqlx::query("ROLLBACK")
                .execute(&mut *transaction)
                .await
                .is_err()
            {
                transaction
                    .connection
                    .close()
                    .await
                    .map_err(|_| unavailable())?;
                return Err(unavailable());
            }
            transaction.return_confirmed().await?;
            Err(error)
        }
    }
}

async fn memory_residency_required(connection: &mut SqliteConnection) -> Result<bool> {
    let rows = sqlx::query("PRAGMA database_list")
        .fetch_all(connection)
        .await
        .map_err(|_| unavailable())?;
    for row in rows {
        let name: String = row.try_get("name").map_err(|_| unavailable())?;
        if name == "main" {
            let file: String = row.try_get("file").map_err(|_| unavailable())?;
            return Ok(file.is_empty());
        }
    }
    Err(unavailable())
}

pub(super) async fn write(
    pool: &SqlitePool,
    table: &str,
    id: &SessionId,
    payload: String,
    expiry: DateTime<Utc>,
    mode: SessionWriteMode,
) -> Result<SessionWriteOutcome> {
    let mut transaction = OwnedSqliteTransaction::begin(pool).await?;
    // 必须在写锁取得之后采样，同宿主时钟保留微秒，不用 SQLite 秒/毫秒函数替代。
    let now = Utc::now();
    let outcome = async {
        let digest = digest_id(id);
        if mode == SessionWriteMode::Update {
            let row = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT expires_at FROM {table} WHERE id=?"
            )))
            .bind(&digest)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| unavailable())?;
            let Some(row) = row else {
                return Ok(SessionWriteOutcome::Missing);
            };
            let previous: Option<DateTime<Utc>> =
                row.try_get("expires_at").map_err(|_| invalid_record())?;
            if previous.is_none_or(|previous| previous <= now) {
                return Ok(SessionWriteOutcome::Missing);
            }
        }
        let expiry = expiry.to_rfc3339_opts(SecondsFormat::Micros, true);
        let query = match mode {
            SessionWriteMode::Insert => format!(
                "INSERT INTO {table} (id,payload,expires_at,last_activity) VALUES (?,?,?,?)"
            ),
            SessionWriteMode::Update => {
                format!("UPDATE {table} SET payload=?,expires_at=?,last_activity=? WHERE id=?")
            }
        };
        let query = sqlx::query(sqlx::AssertSqlSafe(query));
        let query = match mode {
            SessionWriteMode::Insert => query
                .bind(&digest)
                .bind(&payload)
                .bind(expiry)
                .bind(now.timestamp()),
            SessionWriteMode::Update => query
                .bind(&payload)
                .bind(expiry)
                .bind(now.timestamp())
                .bind(&digest),
        };
        match query.execute(&mut *transaction).await {
            Ok(result) if result.rows_affected() > 0 => Ok(SessionWriteOutcome::Written),
            Ok(_) => Ok(if mode == SessionWriteMode::Insert {
                SessionWriteOutcome::Conflict
            } else {
                SessionWriteOutcome::Missing
            }),
            Err(error)
                if mode == SessionWriteMode::Insert
                    && error
                        .as_database_error()
                        .is_some_and(sqlx::error::DatabaseError::is_unique_violation) =>
            {
                Ok(SessionWriteOutcome::Conflict)
            }
            Err(_) => Err(unavailable()),
        }
    }
    .await;
    finish(transaction, outcome).await
}

pub(super) async fn load(
    pool: &SqlitePool,
    table: &str,
    id: &SessionId,
    max_bytes: usize,
    empty_expiry: OffsetDateTime,
) -> Result<Option<SessionRecord>> {
    let maximum = i64::try_from(max_bytes).map_err(|_| limit_error())?;
    let digest = digest_id(id);
    let query = format!(
        "SELECT CASE WHEN length(CAST(payload AS BLOB)) <= ? THEN payload ELSE NULL END AS bounded_payload, length(CAST(payload AS BLOB)) AS payload_length, expires_at FROM {table} WHERE id=?"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(query))
        .bind(maximum)
        .bind(&digest)
        .fetch_optional(pool)
        .await
        .map_err(|_| unavailable())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let length: i64 = row
        .try_get("payload_length")
        .map_err(|_| invalid_record())?;
    if usize::try_from(length).map_err(|_| invalid_record())? > max_bytes {
        return Err(limit_error());
    }
    let payload: String = row
        .try_get("bounded_payload")
        .map_err(|_| invalid_record())?;
    let expiry: DateTime<Utc> = row.try_get("expires_at").map_err(|_| invalid_record())?;
    if expiry <= Utc::now() {
        // 再取得写所有权并重新读取；不能按旧快照删除已经被另一请求续期的行。
        let mut transaction = OwnedSqliteTransaction::begin(pool).await?;
        let now = Utc::now();
        let outcome = async {
            let current = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT expires_at FROM {table} WHERE id=?"
            )))
            .bind(&digest)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| unavailable())?;
            if let Some(current) = current {
                let current: Option<DateTime<Utc>> = current
                    .try_get("expires_at")
                    .map_err(|_| invalid_record())?;
                if current.is_some_and(|current| current <= now) {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "DELETE FROM {table} WHERE id=?"
                    )))
                    .bind(&digest)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|_| unavailable())?;
                }
            }
            Ok(())
        }
        .await;
        finish(transaction, outcome).await?;
        return Ok(None);
    }
    let expiry = to_offset(expiry)?;
    let mut record = SessionRecord::decode(
        id.clone(),
        payload.as_bytes(),
        expiry.min(empty_expiry),
        max_bytes,
    )?;
    record.expiry_date = record.expiry_date.min(expiry);
    Ok(Some(record))
}

pub(super) async fn gc(pool: &SqlitePool, table: &str) -> Result<u64> {
    let mut transaction = OwnedSqliteTransaction::begin(pool).await?;
    let now = Utc::now();
    let outcome = async {
        let mut cursor: Option<String> = None;
        let mut deleted = 0_u64;
        loop {
            // 只取最多 512 个 ID/截止，不读取 payload；NULL 为未启用增强的原生行。
            let rows = if let Some(cursor) = &cursor {
                sqlx::query(sqlx::AssertSqlSafe(format!("SELECT id,expires_at FROM {table} WHERE expires_at IS NOT NULL AND id>? ORDER BY id LIMIT ?")))
                    .bind(cursor).bind(GC_BATCH).fetch_all(&mut *transaction).await
            } else {
                sqlx::query(sqlx::AssertSqlSafe(format!("SELECT id,expires_at FROM {table} WHERE expires_at IS NOT NULL ORDER BY id LIMIT ?")))
                    .bind(GC_BATCH).fetch_all(&mut *transaction).await
            }.map_err(|_| unavailable())?;
            if rows.is_empty() { break; }
            for row in rows {
                let id: String = row.try_get("id").map_err(|_| invalid_record())?;
                let expiry: DateTime<Utc> = row.try_get("expires_at").map_err(|_| invalid_record())?;
                cursor = Some(id.clone());
                if expiry <= now {
                    let result = sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table} WHERE id=?")))
                        .bind(id).execute(&mut *transaction).await.map_err(|_| unavailable())?;
                    deleted = deleted.checked_add(result.rows_affected()).ok_or_else(unavailable)?;
                }
            }
        }
        Ok(deleted)
    }.await;
    finish(transaction, outcome).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> Result<SqlitePool> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:"))
            .await
            .map_err(|_| unavailable())?;
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY,payload TEXT,last_activity INTEGER,expires_at TEXT)")
            .execute(&pool).await.map_err(|_| unavailable())?;
        Ok(pool)
    }

    #[tokio::test]
    async fn sqlite_guarded_keeps_atomic_modes_and_parses_offset_deadlines() -> Result<()> {
        let pool = test_pool().await?;
        let id = SessionId::generate()?;
        let expiry = OffsetDateTime::now_utc() + time::Duration::hours(1);
        let expiry = expiry
            .replace_nanosecond(expiry.nanosecond() / 1000 * 1000)
            .map_err(|_| invalid_record())?;
        let record = SessionRecord {
            id: id.clone(),
            data: serde_json::json!({"guarded":"retained"}),
            expiry_date: expiry,
        };
        let chrono_expiry =
            DateTime::<Utc>::from_timestamp(expiry.unix_timestamp(), expiry.nanosecond())
                .ok_or_else(invalid_record)?;
        let payload = String::from_utf8(record.encode(4096)?).map_err(|_| invalid_record())?;
        assert_eq!(
            write(
                &pool,
                "sessions",
                &id,
                payload.clone(),
                chrono_expiry,
                SessionWriteMode::Insert
            )
            .await?,
            SessionWriteOutcome::Written
        );
        assert_eq!(
            write(
                &pool,
                "sessions",
                &id,
                payload.clone(),
                chrono_expiry,
                SessionWriteMode::Insert
            )
            .await?,
            SessionWriteOutcome::Conflict
        );
        let timezone = chrono::FixedOffset::east_opt(8 * 3600).ok_or_else(invalid_record)?;
        let offset_expiry = chrono_expiry
            .with_timezone(&timezone)
            .to_rfc3339_opts(SecondsFormat::Micros, false);
        sqlx::query("UPDATE sessions SET expires_at=? WHERE id=?")
            .bind(offset_expiry)
            .bind(digest_id(&id))
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        let loaded = load(&pool, "sessions", &id, 4096, expiry)
            .await?
            .ok_or_else(invalid_record)?;
        assert_eq!(loaded.expiry_date, expiry);
        assert_eq!(loaded.data, record.data);
        let elapsed = Utc::now() - chrono::Duration::microseconds(1);
        sqlx::query("UPDATE sessions SET expires_at=? WHERE id=?")
            .bind(
                elapsed
                    .with_timezone(&timezone)
                    .to_rfc3339_opts(SecondsFormat::Micros, false),
            )
            .bind(digest_id(&id))
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(
            write(
                &pool,
                "sessions",
                &id,
                payload.clone(),
                chrono_expiry,
                SessionWriteMode::Update
            )
            .await?,
            SessionWriteOutcome::Missing
        );
        let stored: String = sqlx::query_scalar("SELECT payload FROM sessions WHERE id=?")
            .bind(digest_id(&id))
            .fetch_one(&pool)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(stored, payload);
        assert!(load(&pool, "sessions", &id, 4096, expiry).await?.is_none());
        assert_eq!(
            write(
                &pool,
                "sessions",
                &id,
                payload,
                chrono_expiry,
                SessionWriteMode::Update
            )
            .await?,
            SessionWriteOutcome::Missing
        );
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn sqlite_guarded_gc_pages_without_payload_and_rolls_back_on_invalid_deadline()
    -> Result<()> {
        let pool = test_pool().await?;
        let past =
            (Utc::now() - chrono::Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Micros, true);
        let future =
            (Utc::now() + chrono::Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Micros, true);
        let mut transaction = pool.begin().await.map_err(|_| unavailable())?;
        for index in 0..515 {
            sqlx::query("INSERT INTO sessions(id,expires_at) VALUES (?,?)")
                .bind(format!("{index:064x}"))
                .bind(if index < 513 { &past } else { &future })
                .execute(&mut *transaction)
                .await
                .map_err(|_| unavailable())?;
        }
        sqlx::query(
            "INSERT INTO sessions(id,expires_at) VALUES ('native',NULL),('z-invalid','malformed')",
        )
        .execute(&mut *transaction)
        .await
        .map_err(|_| unavailable())?;
        transaction.commit().await.map_err(|_| unavailable())?;
        assert!(gc(&pool, "sessions").await.is_err());
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(remaining, 517);
        sqlx::query("DELETE FROM sessions WHERE id='z-invalid'")
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(gc(&pool, "sessions").await?, 513);
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(remaining, 3);
        pool.close().await;
        Ok(())
    }

    async fn file_pool(path: &std::path::Path) -> Result<SqlitePool> {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(path)
                    .foreign_keys(true),
            )
            .await
            .map_err(|_| unavailable())
    }

    #[tokio::test]
    async fn guarded_owner_commit_error_discards_physical_connection_and_waits_for_rollback()
    -> Result<()> {
        let file = tempfile::NamedTempFile::new().map_err(|_| unavailable())?;
        let pool = file_pool(file.path()).await?;
        sqlx::query("CREATE TABLE parent(id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        sqlx::query("CREATE TABLE child(parent_id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED)").execute(&pool).await.map_err(|_| unavailable())?;
        sqlx::query("CREATE TEMP TABLE physical_sentinel(value INTEGER)")
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        let mut transaction = OwnedSqliteTransaction::begin(&pool).await?;
        sqlx::query("INSERT INTO child VALUES(404)")
            .execute(&mut *transaction)
            .await
            .map_err(|_| unavailable())?;
        assert!(finish(transaction, Ok(())).await.is_err());
        assert_eq!(pool.size(), 0);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM child")
            .fetch_one(&pool)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(count, 0);
        assert!(
            sqlx::query("SELECT * FROM physical_sentinel")
                .fetch_all(&pool)
                .await
                .is_err()
        );
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn guarded_owner_rollback_error_never_returns_unknown_connection_to_pool() -> Result<()> {
        let file = tempfile::NamedTempFile::new().map_err(|_| unavailable())?;
        let pool = file_pool(file.path()).await?;
        sqlx::query("CREATE TABLE durable(value INTEGER)")
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        sqlx::query("CREATE TEMP TABLE physical_sentinel(value INTEGER)")
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        let mut transaction = OwnedSqliteTransaction::begin(&pool).await?;
        // 注入事务状态丢失：finish 的 ROLLBACK 必须原生失败并触发物理关闭。
        sqlx::query("ROLLBACK")
            .execute(&mut *transaction)
            .await
            .map_err(|_| unavailable())?;
        assert!(
            finish::<()>(transaction, Err(invalid_record()))
                .await
                .is_err()
        );
        assert_eq!(pool.size(), 0);
        assert!(
            sqlx::query("SELECT * FROM physical_sentinel")
                .fetch_all(&pool)
                .await
                .is_err()
        );
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn dropping_guarded_owner_closes_instead_of_repooling_an_open_transaction() -> Result<()>
    {
        let file = tempfile::NamedTempFile::new().map_err(|_| unavailable())?;
        let pool = file_pool(file.path()).await?;
        sqlx::query("CREATE TABLE durable(value INTEGER)")
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        sqlx::query("CREATE TEMP TABLE physical_sentinel(value INTEGER)")
            .execute(&pool)
            .await
            .map_err(|_| unavailable())?;
        let mut transaction = OwnedSqliteTransaction::begin(&pool).await?;
        sqlx::query("INSERT INTO durable VALUES(17)")
            .execute(&mut *transaction)
            .await
            .map_err(|_| unavailable())?;
        drop(transaction);
        // 单连接许可要等原 physical close 释放，才会得到新连接；不把 Drop 当同步回滚。
        let mut next = tokio::time::timeout(std::time::Duration::from_secs(2), pool.acquire())
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM durable")
            .fetch_one(&mut *next)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(count, 0);
        assert!(
            sqlx::query("SELECT * FROM physical_sentinel")
                .fetch_all(&mut *next)
                .await
                .is_err()
        );
        drop(next);
        pool.close().await;
        Ok(())
    }

    async fn release_controlled_pool(
        options: sqlx::sqlite::SqliteConnectOptions,
    ) -> Result<(SqlitePool, std::sync::Arc<std::sync::atomic::AtomicBool>)> {
        release_controlled_pool_with_max(options, 1).await
    }

    async fn release_controlled_pool_with_max(
        options: sqlx::sqlite::SqliteConnectOptions,
        maximum: u32,
    ) -> Result<(SqlitePool, std::sync::Arc<std::sync::atomic::AtomicBool>)> {
        let reject = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = reject.clone();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(maximum)
            .min_connections(0)
            .idle_timeout(None)
            .max_lifetime(None)
            .after_release(move |_, _| {
                let allowed = !signal.load(std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move { Ok(allowed) })
            })
            .connect_with(options)
            .await
            .map_err(|_| unavailable())?;
        let mut setup = pool.acquire().await.map_err(|_| unavailable())?;
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY,payload TEXT,last_activity INTEGER,expires_at TEXT)")
            .execute(&mut *setup).await.map_err(|_| unavailable())?;
        sqlx::query("INSERT INTO sessions VALUES('dead','{}',0,'2000-01-01T00:00:00.000000Z')")
            .execute(&mut *setup)
            .await
            .map_err(|_| unavailable())?;
        assert!(crate::database::migration::return_sqlite_connection(setup).await?);
        Ok((pool, reject))
    }

    #[tokio::test]
    async fn guarded_memory_commit_cannot_report_written_when_after_release_destroys_last_handle()
    -> Result<()> {
        let (pool, reject) =
            release_controlled_pool(sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:"))
                .await?;
        reject.store(true, std::sync::atomic::Ordering::SeqCst);
        let id = SessionId::generate()?;
        let expiry = Utc::now() + chrono::Duration::hours(1);
        assert!(
            write(
                &pool,
                "sessions",
                &id,
                "{}".into(),
                expiry,
                SessionWriteMode::Insert
            )
            .await
            .is_err()
        );
        assert!(pool.is_closed());
        assert!(pool.acquire().await.is_err());
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn guarded_memory_gc_cannot_report_success_when_after_release_destroys_last_handle()
    -> Result<()> {
        let (pool, reject) =
            release_controlled_pool(sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:"))
                .await?;
        reject.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(gc(&pool, "sessions").await.is_err());
        assert!(pool.is_closed());
        assert!(pool.acquire().await.is_err());
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn file_handle_recycling_after_commit_and_gc_preserves_success_and_data() -> Result<()> {
        let file = tempfile::NamedTempFile::new().map_err(|_| unavailable())?;
        let (pool, reject) = release_controlled_pool(
            sqlx::sqlite::SqliteConnectOptions::new().filename(file.path()),
        )
        .await?;
        reject.store(true, std::sync::atomic::Ordering::SeqCst);
        let id = SessionId::generate()?;
        let expiry = Utc::now() + chrono::Duration::hours(1);
        assert_eq!(
            write(
                &pool,
                "sessions",
                &id,
                "{}".into(),
                expiry,
                SessionWriteMode::Insert
            )
            .await?,
            SessionWriteOutcome::Written
        );
        assert!(!pool.is_closed());
        assert_eq!(gc(&pool, "sessions").await?, 1);
        assert!(!pool.is_closed());
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE id=?")
            .bind(digest_id(&id))
            .fetch_one(&pool)
            .await
            .map_err(|_| unavailable())?;
        assert_eq!(rows, 1);
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn named_shared_memory_with_one_actual_handle_cannot_hide_residency_failure() -> Result<()>
    {
        for maximum in [1, 2] {
            for collect in [false, true] {
                let name = SessionId::generate()?;
                let options = sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(format!(
                        "file:guarded-shared-{}?mode=memory&cache=shared",
                        name.as_str()
                    ))
                    .create_if_missing(true)
                    .shared_cache(true);
                let (pool, reject) = release_controlled_pool_with_max(options, maximum).await?;
                // max2 只是上限；此 fixture 刻意只有一个实际 handle，min0 没有锚点。
                assert_eq!(pool.size(), 1);
                assert_eq!(pool.options().get_max_connections(), maximum);
                assert_eq!(pool.options().get_min_connections(), 0);
                reject.store(true, std::sync::atomic::Ordering::SeqCst);
                if collect {
                    assert!(gc(&pool, "sessions").await.is_err());
                } else {
                    let expiry = Utc::now() + chrono::Duration::hours(1);
                    assert!(
                        write(
                            &pool,
                            "sessions",
                            &SessionId::generate()?,
                            "{}".into(),
                            expiry,
                            SessionWriteMode::Insert
                        )
                        .await
                        .is_err()
                    );
                }
                assert!(pool.is_closed());
                assert!(pool.acquire().await.is_err());
                pool.close().await;
            }
        }
        Ok(())
    }
}
