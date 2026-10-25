//! PostgreSQL/MySQL atomic fixed-window rate-limit storage.

use crate::core::{RateLimitBucket, RateLimitFuture, RateLimitStore, Result};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::db::{DatabaseConnection, Db};

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

/// Creates the shared fixed-window rate-limit table.
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
    let names = crate::db::DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "database rate-limit migration requires MigrationPlugin or TestDatabase scope".into(),
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

/// Shared-database store for [`crate::core::RateLimiter`].
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
    /// Creates a store using the application's configured shared database and table resolver.
    pub fn new(db: Db) -> Self {
        Self {
            db,
            cleanup_counter: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl RateLimitStore for DatabaseRateLimitStore {
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
                _ => Err(crate::core::Error::custom(
                    "unsupported_rate_limit_backend",
                    "database rate limiting requires PostgreSQL or MySQL",
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
        "INSERT INTO {table} (key_hash, window_started_at, window_seconds, hit_count) VALUES ($1, (FLOOR(EXTRACT(EPOCH FROM CURRENT_TIMESTAMP) / CAST($2 AS NUMERIC)) * CAST($2 AS NUMERIC))::BIGINT, CAST($2 AS BIGINT), 1) ON CONFLICT (key_hash) DO UPDATE SET hit_count = CASE WHEN {table}.window_started_at = EXCLUDED.window_started_at AND {table}.window_seconds = EXCLUDED.window_seconds THEN LEAST({table}.hit_count + 1, 9223372036854775807) ELSE 1 END, window_started_at = EXCLUDED.window_started_at, window_seconds = EXCLUDED.window_seconds RETURNING hit_count, GREATEST(1, CEIL(window_started_at + window_seconds - EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)))::BIGINT AS reset_after_secs"
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
        "INSERT INTO {table} (key_hash, window_started_at, window_seconds, hit_count) VALUES (?, FLOOR(UNIX_TIMESTAMP(CURRENT_TIMESTAMP) / ?) * ?, ?, 1) ON DUPLICATE KEY UPDATE hit_count = IF(window_started_at = VALUES(window_started_at) AND window_seconds = VALUES(window_seconds), LEAST(hit_count + 1, 9223372036854775807), 1), window_started_at = VALUES(window_started_at), window_seconds = VALUES(window_seconds)"
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
        _ => {
            return Err(crate::core::Error::custom(
                "unsupported_rate_limit_backend",
                "database rate limiting requires PostgreSQL or MySQL",
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
