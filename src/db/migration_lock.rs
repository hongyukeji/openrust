//! `MigrationServiceProvider` 使用的 PostgreSQL/MySQL 执行互斥。

use std::time::{Duration, Instant};

use crate::core::{Error, Result};
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use sea_orm::RuntimeErr;
use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement, TransactionTrait};

use super::{DatabaseConnection, Db};

const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// 迁移期间持有的事务行锁。
pub(super) struct MigrationLock {
    transaction: sea_orm::DatabaseTransaction,
}

impl MigrationLock {
    /// 取得迁移锁；竞争时以短间隔重试，超过预算后失败关闭。
    pub(super) async fn acquire(db: &Db, wait_timeout: Duration) -> Result<Self> {
        let connection = db.primary_connection()?;
        let backend = connection.get_database_backend();
        let table = db.quoted_table_name("migration_locks")?;
        ensure_lock_table(&connection, backend, &table).await?;
        let started = Instant::now();

        loop {
            let transaction = connection.begin().await.map_err(Error::internal)?;
            match transaction
                .query_one_raw(Statement::from_string(
                    backend,
                    format!("SELECT lock_id FROM {table} WHERE lock_id = 1 FOR UPDATE NOWAIT"),
                ))
                .await
            {
                Ok(Some(_)) => return Ok(Self { transaction }),
                Ok(None) => {
                    let _ = transaction.rollback().await;
                    return Err(Error::custom(
                        "migration_lock_row_missing",
                        "迁移锁表中的固定锁行缺失",
                    ));
                }
                Err(error) if is_lock_contention(&error, backend) => {
                    transaction.rollback().await.map_err(Error::internal)?;
                    let remaining = wait_timeout.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        return Err(Error::custom(
                            "migration_lock_timeout",
                            "等待数据库迁移锁超时",
                        ));
                    }
                    tokio::time::sleep(remaining.min(RETRY_INTERVAL)).await;
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(Error::internal(error));
                }
            }
        }
    }

    /// 提交释放锁事务。
    pub(super) async fn release(self) -> Result<()> {
        self.transaction.commit().await.map_err(Error::internal)
    }
}

async fn ensure_lock_table(
    connection: &DatabaseConnection,
    backend: DbBackend,
    table: &str,
) -> Result<()> {
    let create = match backend {
        DbBackend::Postgres => {
            format!("CREATE TABLE IF NOT EXISTS {table} (lock_id SMALLINT PRIMARY KEY)")
        }
        DbBackend::MySql => format!(
            "CREATE TABLE IF NOT EXISTS {table} (lock_id SMALLINT PRIMARY KEY) ENGINE=InnoDB"
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_database_backend",
                "迁移互斥仅支持 PostgreSQL 和 MySQL",
            ));
        }
    };
    connection
        .execute_raw(Statement::from_string(backend, create))
        .await
        .map_err(Error::internal)?;
    if connection
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT lock_id FROM {table} WHERE lock_id = 1"),
        ))
        .await
        .map_err(Error::internal)?
        .is_some()
    {
        return Ok(());
    }
    let insert = match backend {
        DbBackend::Postgres => {
            format!("INSERT INTO {table} (lock_id) VALUES (1) ON CONFLICT (lock_id) DO NOTHING")
        }
        DbBackend::MySql => format!("INSERT IGNORE INTO {table} (lock_id) VALUES (1)"),
        _ => {
            return Err(Error::custom(
                "unsupported_database_backend",
                "迁移互斥仅支持 PostgreSQL 和 MySQL",
            ));
        }
    };
    connection
        .execute_raw(Statement::from_string(backend, insert))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn is_lock_contention(error: &DbErr, backend: DbBackend) -> bool {
    let (DbErr::Query(RuntimeErr::SqlxError(runtime_error))
    | DbErr::Exec(RuntimeErr::SqlxError(runtime_error))) = error
    else {
        return false;
    };
    let sea_orm::sqlx::Error::Database(database_error) = runtime_error.as_ref() else {
        return false;
    };
    match backend {
        #[cfg(feature = "db-postgres")]
        DbBackend::Postgres => database_error.code().as_deref() == Some("55P03"),
        #[cfg(feature = "db-mysql")]
        DbBackend::MySql => database_error
            .try_downcast_ref::<sea_orm::sqlx::mysql::MySqlDatabaseError>()
            .is_some_and(|error| error.number() == 3572),
        _ => false,
    }
}

#[cfg(not(any(feature = "db-postgres", feature = "db-mysql")))]
fn is_lock_contention(_error: &DbErr, _backend: DbBackend) -> bool {
    false
}

pub(super) const fn default_wait_timeout() -> Duration {
    DEFAULT_WAIT_TIMEOUT
}
