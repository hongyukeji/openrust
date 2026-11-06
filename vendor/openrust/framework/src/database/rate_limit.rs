//! PostgreSQL、MySQL与SQLite的原子固定窗口限流存储。

use crate::core::{RateLimitBucket, RateLimitFuture, RateLimitStore, Result};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::database::{DatabaseConnection, Db};

const CLEANUP_EVERY: u64 = 256;
const CLEANUP_BATCH_LIMIT: u16 = 512;

#[derive(DeriveIden)]
#[allow(dead_code)]
enum RateLimitTable {
    Table,
    KeyHash,
    WindowStartedAt,
    WindowSeconds,
    HitCount,
}

/// 创建共享固定窗口限流表。
pub struct DatabaseRateLimitMigration;

impl MigrationName for DatabaseRateLimitMigration {
    fn name(&self) -> &'static str {
        "m20261026_000003_openrust_rate_limits"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DatabaseRateLimitMigration {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let table = migration_table_alias("rate_limits", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RateLimitTable::KeyHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(RateLimitTable::WindowStartedAt)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RateLimitTable::WindowSeconds)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RateLimitTable::HitCount)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let table = migration_table_alias("rate_limits", manager.get_database_backend())?;
        manager
            .drop_table(Table::drop().table(table).if_exists().to_owned())
            .await
    }
}

fn migration_table_alias(
    logical_name: &str,
    backend: DbBackend,
) -> std::result::Result<Alias, DbErr> {
    let names = crate::database::DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "database rate-limit migration requires MigrationServiceProvider or TestDatabase scope"
                .into(),
        )
    })?;
    names
        .quote(logical_name, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(logical_name)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

/// 为 [`crate::core::RateLimiter`] 提供的共享数据库存储。
#[derive(Clone)]
pub struct DatabaseRateLimitStore {
    db: Db,
    cleanup_counter: Arc<AtomicU64>,
}

impl std::fmt::Debug for DatabaseRateLimitStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseRateLimitStore")
    }
}

impl DatabaseRateLimitStore {
    /// 使用应用配置的共享数据库和表名解析器创建存储。
    pub fn new(db: Db) -> Self {
        Self {
            db,
            cleanup_counter: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl RateLimitStore for DatabaseRateLimitStore {
    fn hit<'a>(
        &'a self,
        key: &'a str,
        decay: std::time::Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
        Box::pin(async move {
            let connection = self.db.connection()?;
            let table = self.db.quoted_table_name("rate_limits")?;
            hit_decay(&connection, &table, &digest(key), decay).await
        })
    }
    fn attempts<'a>(
        &'a self,
        key: &'a str,
    ) -> RateLimitFuture<'a, Result<Option<RateLimitBucket>>> {
        Box::pin(async move {
            let connection = self.db.connection()?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("rate_limits")?;
            let (now, placeholder) = decay_sql(backend)?;
            let sql = format!(
                "SELECT hit_count, window_started_at + window_seconds - {now} AS reset_after_secs FROM {table} WHERE key_hash = {placeholder} AND window_started_at + window_seconds > {now}"
            );
            let row = connection
                .query_one_raw(Statement::from_sql_and_values(
                    backend,
                    sql,
                    [digest(key).into()],
                ))
                .await
                .map_err(crate::Error::internal)?;
            row.as_ref().map(read_bucket).transpose()
        })
    }
    fn clear<'a>(&'a self, key: &'a str) -> RateLimitFuture<'a, Result<()>> {
        Box::pin(async move {
            let connection = self.db.connection()?;
            let backend = connection.get_database_backend();
            let (_, placeholder) = decay_sql(backend)?;
            let table = self.db.quoted_table_name("rate_limits")?;
            connection
                .execute_raw(Statement::from_sql_and_values(
                    backend,
                    format!("DELETE FROM {table} WHERE key_hash = {placeholder}"),
                    [digest(key).into()],
                ))
                .await
                .map_err(crate::Error::internal)?;
            Ok(())
        })
    }
    fn consume<'a>(
        &'a self,
        key: &'a str,
        window: std::time::Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
        Box::pin(async move {
            let key_hash = digest(key);
            let connection = self.db.connection()?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("rate_limits")?;
            if self
                .cleanup_counter
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(CLEANUP_EVERY)
            {
                prune_expired(&connection, &table, backend).await?;
            }
            match backend {
                DbBackend::Postgres => {
                    consume_postgres(&connection, &table, &key_hash, window).await
                }
                DbBackend::MySql => consume_mysql(&connection, &table, &key_hash, window).await,
                DbBackend::Sqlite => consume_sqlite(&connection, &table, &key_hash, window).await,
                _ => Err(crate::core::Error::custom(
                    "unsupported_rate_limit_backend",
                    "database rate limiting requires PostgreSQL, MySQL or SQLite",
                )),
            }
        })
    }
}

async fn consume_postgres(
    connection: &DatabaseConnection,
    table: &str,
    key_hash: &str,
    window: std::time::Duration,
) -> Result<RateLimitBucket> {
    let sql = format!(
        "INSERT INTO {table} (key_hash, window_started_at, window_seconds, hit_count) VALUES ($1, (FLOOR(EXTRACT(EPOCH FROM CURRENT_TIMESTAMP) / CAST($2 AS NUMERIC)) * CAST($2 AS NUMERIC))::BIGINT, CAST($2 AS BIGINT), 1) ON CONFLICT (key_hash) DO UPDATE SET hit_count = CASE WHEN {table}.window_started_at = EXCLUDED.window_started_at AND {table}.window_seconds = EXCLUDED.window_seconds THEN CASE WHEN {table}.hit_count < 9223372036854775807 THEN {table}.hit_count + 1 ELSE 9223372036854775807 END ELSE 1 END, window_started_at = EXCLUDED.window_started_at, window_seconds = EXCLUDED.window_seconds RETURNING hit_count, GREATEST(1, CEIL(window_started_at + window_seconds - EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)))::BIGINT AS reset_after_secs"
    );
    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            [key_hash.to_owned().into(), to_i64(window.as_secs())?.into()],
        ))
        .await
        .map_err(crate::core::Error::internal)?
        .ok_or_else(|| crate::core::Error::internal(RateLimitStoreError))?;
    let count = row
        .try_get::<i64>("", "hit_count")
        .map_err(crate::core::Error::internal)?;
    let reset_after = row
        .try_get::<i64>("", "reset_after_secs")
        .map_err(crate::core::Error::internal)?;
    Ok(RateLimitBucket::new(to_u64(count)?, to_u64(reset_after)?))
}

async fn consume_mysql(
    connection: &DatabaseConnection,
    table: &str,
    key_hash: &str,
    window: std::time::Duration,
) -> Result<RateLimitBucket> {
    let transaction = connection
        .begin()
        .await
        .map_err(crate::core::Error::internal)?;
    let seconds = to_i64(window.as_secs())?;
    let upsert = format!(
        "INSERT INTO {table} (key_hash, window_started_at, window_seconds, hit_count) VALUES (?, FLOOR(UNIX_TIMESTAMP(CURRENT_TIMESTAMP) / ?) * ?, ?, 1) ON DUPLICATE KEY UPDATE hit_count = IF(window_started_at = VALUES(window_started_at) AND window_seconds = VALUES(window_seconds), CASE WHEN hit_count < 9223372036854775807 THEN hit_count + 1 ELSE 9223372036854775807 END, 1), window_started_at = VALUES(window_started_at), window_seconds = VALUES(window_seconds)"
    );
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::MySql,
            upsert,
            [
                key_hash.to_owned().into(),
                seconds.into(),
                seconds.into(),
                seconds.into(),
            ],
        ))
        .await
        .map_err(crate::core::Error::internal)?;
    let select = format!(
        "SELECT hit_count, GREATEST(1, CEIL(window_started_at + window_seconds - UNIX_TIMESTAMP(CURRENT_TIMESTAMP))) AS reset_after_secs FROM {table} WHERE key_hash = ? FOR UPDATE"
    );
    let row = transaction
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::MySql,
            select,
            [key_hash.to_owned().into()],
        ))
        .await
        .map_err(crate::core::Error::internal)?
        .ok_or_else(|| crate::core::Error::internal(RateLimitStoreError))?;
    let count = row
        .try_get::<i64>("", "hit_count")
        .map_err(crate::core::Error::internal)?;
    let reset_after = row
        .try_get::<i64>("", "reset_after_secs")
        .map_err(crate::core::Error::internal)?;
    transaction
        .commit()
        .await
        .map_err(crate::core::Error::internal)?;
    Ok(RateLimitBucket::new(to_u64(count)?, to_u64(reset_after)?))
}

async fn consume_sqlite(
    connection: &DatabaseConnection,
    table: &str,
    key_hash: &str,
    window: std::time::Duration,
) -> Result<RateLimitBucket> {
    let seconds = to_i64(window.as_secs())?;
    if seconds <= 0 {
        return Err(crate::Error::BadRequest("限流窗口必须至少一秒".into()));
    }
    let transaction = super::begin_for_update(connection).await?;
    let outcome = async {
        // 在取得写所有权后从同一SQLite引擎采样，整数秒与ceil(窗口截止-now)一致。
        let row = transaction
            .query_one_raw(Statement::from_string(DbBackend::Sqlite,
                "SELECT CAST(strftime('%s','now') AS INTEGER) AS now_seconds"))
            .await.map_err(crate::Error::internal)?
            .ok_or_else(|| crate::Error::internal(RateLimitStoreError))?;
        let now: i64 = row.try_get("", "now_seconds").map_err(crate::Error::internal)?;
        let started = now - now.rem_euclid(seconds);
        let end = started.checked_add(seconds).ok_or_else(|| crate::Error::BadRequest("限流窗口超出时间范围".into()))?;
        transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("INSERT INTO {table} (key_hash,window_started_at,window_seconds,hit_count) VALUES (?,?,?,1) ON CONFLICT (key_hash) DO UPDATE SET hit_count=CASE WHEN {table}.window_started_at=excluded.window_started_at AND {table}.window_seconds=excluded.window_seconds THEN CASE WHEN {table}.hit_count<9223372036854775807 THEN {table}.hit_count+1 ELSE 9223372036854775807 END ELSE 1 END,window_started_at=excluded.window_started_at,window_seconds=excluded.window_seconds"),
            [key_hash.to_owned().into(),started.into(),seconds.into()]))
            .await.map_err(crate::Error::internal)?;
        let row=transaction.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("SELECT hit_count FROM {table} WHERE key_hash=?"),[key_hash.to_owned().into()]))
            .await.map_err(crate::Error::internal)?
            .ok_or_else(|| crate::Error::internal(RateLimitStoreError))?;
        let count:i64=row.try_get("","hit_count").map_err(crate::Error::internal)?;
        Ok(RateLimitBucket::new(to_u64(count)?,to_u64(std::cmp::max(end-now,1))?))
    }.await;
    match outcome {
        Ok(bucket) => {
            transaction.commit().await.map_err(crate::Error::internal)?;
            Ok(bucket)
        }
        Err(error) => {
            transaction
                .rollback()
                .await
                .map_err(crate::Error::internal)?;
            Err(error)
        }
    }
}

async fn prune_expired(
    connection: &DatabaseConnection,
    table: &str,
    backend: DbBackend,
) -> Result<()> {
    let (sql, backend) = match backend {
        DbBackend::Postgres => (
            format!(
                "DELETE FROM {table} WHERE key_hash IN (SELECT key_hash FROM {table} WHERE window_started_at + window_seconds <= FLOOR(EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)) LIMIT {CLEANUP_BATCH_LIMIT} FOR UPDATE SKIP LOCKED)"
            ),
            DbBackend::Postgres,
        ),
        DbBackend::MySql => (
            format!(
                "DELETE FROM {table} WHERE window_started_at + window_seconds <= FLOOR(UNIX_TIMESTAMP(CURRENT_TIMESTAMP)) ORDER BY window_started_at LIMIT {CLEANUP_BATCH_LIMIT}"
            ),
            DbBackend::MySql,
        ),
        DbBackend::Sqlite => (
            format!(
                "DELETE FROM {table} WHERE key_hash IN (SELECT key_hash FROM {table} WHERE window_started_at + window_seconds <= CAST(strftime('%s','now') AS INTEGER) ORDER BY window_started_at LIMIT {CLEANUP_BATCH_LIMIT})"
            ),
            DbBackend::Sqlite,
        ),
        _ => {
            return Err(crate::core::Error::custom(
                "unsupported_rate_limit_backend",
                "database rate limiting requires PostgreSQL, MySQL or SQLite",
            ));
        }
    };
    connection
        .execute_raw(Statement::from_string(backend, sql))
        .await
        .map_err(crate::core::Error::internal)?;
    Ok(())
}

fn digest(key: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(key.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn to_i64(value: u64) -> Result<i64> {
    <i64 as std::convert::TryFrom<u64>>::try_from(value).map_err(crate::core::Error::internal)
}

fn to_u64(value: i64) -> Result<u64> {
    <u64 as std::convert::TryFrom<i64>>::try_from(value).map_err(crate::core::Error::internal)
}

#[derive(Debug)]
struct RateLimitStoreError;

impl std::fmt::Display for RateLimitStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("database rate-limit upsert returned no counter")
    }
}

impl std::error::Error for RateLimitStoreError {}

#[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
mod real_server_tests;
#[cfg(all(test, feature = "db-sqlite"))]
mod tests;

fn decay_sql(backend: DbBackend) -> Result<(&'static str, &'static str)> {
    match backend {
        DbBackend::Postgres => Ok((
            "CAST(FLOOR(EXTRACT(EPOCH FROM clock_timestamp())) AS BIGINT)",
            "$1",
        )),
        DbBackend::MySql => Ok(("CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP) AS SIGNED)", "?")),
        DbBackend::Sqlite => Ok(("CAST(strftime('%s','now') AS INTEGER)", "?")),
        _ => Err(crate::Error::custom(
            "unsupported_rate_limit_backend",
            "database rate limiting requires PostgreSQL, MySQL or SQLite",
        )),
    }
}
fn read_bucket(row: &sea_orm::QueryResult) -> Result<RateLimitBucket> {
    let count: i64 = row
        .try_get("", "hit_count")
        .map_err(crate::Error::internal)?;
    let reset: i64 = row
        .try_get("", "reset_after_secs")
        .map_err(crate::Error::internal)?;
    Ok(RateLimitBucket::new(to_u64(count)?, to_u64(reset)?))
}
async fn hit_decay(
    connection: &DatabaseConnection,
    table: &str,
    key_hash: &str,
    decay: std::time::Duration,
) -> Result<RateLimitBucket> {
    let seconds = to_i64(decay.as_secs())?;
    if seconds <= 0 || seconds > 31_536_000 || decay.subsec_nanos() != 0 {
        return Err(crate::Error::custom(
            "invalid_rate_limit_policy",
            "限流衰减窗口必须为正整数秒且不超过一年",
        ));
    }
    let backend = connection.get_database_backend();
    let (now, placeholder) = decay_sql(backend)?;
    let transaction = super::begin_for_update(connection).await?;
    let outcome = async {
        // SQLite先取得写所有权再采样；其他后端同一连接原子upsert锁定冲突行。
        let upsert = match backend {
            DbBackend::Postgres => format!("INSERT INTO {table} (key_hash,window_started_at,window_seconds,hit_count) VALUES ($1,{now},$2,1) ON CONFLICT (key_hash) DO UPDATE SET hit_count=CASE WHEN {table}.window_started_at+{table}.window_seconds>EXCLUDED.window_started_at THEN CASE WHEN {table}.hit_count<9223372036854775807 THEN {table}.hit_count+1 ELSE 9223372036854775807 END ELSE 1 END,window_seconds=CASE WHEN {table}.window_started_at+{table}.window_seconds>EXCLUDED.window_started_at THEN {table}.window_seconds ELSE EXCLUDED.window_seconds END,window_started_at=CASE WHEN {table}.window_started_at+{table}.window_seconds>EXCLUDED.window_started_at THEN {table}.window_started_at ELSE EXCLUDED.window_started_at END"),
            DbBackend::Sqlite => format!("INSERT INTO {table} (key_hash,window_started_at,window_seconds,hit_count) VALUES (?,{now},?,1) ON CONFLICT (key_hash) DO UPDATE SET hit_count=CASE WHEN {table}.window_started_at+{table}.window_seconds>excluded.window_started_at THEN CASE WHEN {table}.hit_count<9223372036854775807 THEN {table}.hit_count+1 ELSE 9223372036854775807 END ELSE 1 END,window_seconds=CASE WHEN {table}.window_started_at+{table}.window_seconds>excluded.window_started_at THEN {table}.window_seconds ELSE excluded.window_seconds END,window_started_at=CASE WHEN {table}.window_started_at+{table}.window_seconds>excluded.window_started_at THEN {table}.window_started_at ELSE excluded.window_started_at END"),
            // MySQL赋值从左到右：先根据旧到期时间更新count/seconds，最后由重置后的count=1决定新起点。
            DbBackend::MySql => format!("INSERT INTO {table} (key_hash,window_started_at,window_seconds,hit_count) VALUES (?,{now},?,1) ON DUPLICATE KEY UPDATE hit_count=IF(window_started_at+window_seconds>VALUES(window_started_at),CASE WHEN hit_count<9223372036854775807 THEN hit_count+1 ELSE 9223372036854775807 END,1),window_seconds=IF(window_started_at+window_seconds>VALUES(window_started_at),window_seconds,VALUES(window_seconds)),window_started_at=IF(hit_count=1,VALUES(window_started_at),window_started_at)"),
            _ => return Err(crate::Error::custom("unsupported_rate_limit_backend", "不支持的限流数据库")),
        };
        transaction.execute_raw(Statement::from_sql_and_values(backend, upsert, [key_hash.to_owned().into(), seconds.into()])).await.map_err(crate::Error::internal)?;
        let sql = format!("SELECT hit_count, CASE WHEN window_started_at+window_seconds>{now} THEN window_started_at+window_seconds-{now} ELSE 1 END AS reset_after_secs FROM {table} WHERE key_hash={placeholder}");
        let row = transaction.query_one_raw(Statement::from_sql_and_values(backend, sql, [key_hash.to_owned().into()])).await.map_err(crate::Error::internal)?.ok_or_else(|| crate::Error::internal(RateLimitStoreError))?;
        read_bucket(&row)
    }.await;
    match outcome {
        Ok(bucket) => {
            transaction.commit().await.map_err(crate::Error::internal)?;
            Ok(bucket)
        }
        Err(error) => {
            transaction
                .rollback()
                .await
                .map_err(crate::Error::internal)?;
            Err(error)
        }
    }
}
