//! `MigrationServiceProvider` 使用的 PostgreSQL/MySQL 执行互斥。

use std::time::Duration;

use crate::core::{Error, Result};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DbBackend, Statement, TransactionTrait};

use super::{DatabaseConnection, Db};

const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_millis(100);

// 获取阶段也必须有所有者：调用方取消 SQL/GET_LOCK future 时，独立会话仍需关闭。
struct PendingConnection(Option<DatabaseConnection>);

#[cfg(test)]
struct AcquireGate {
    entered: std::sync::Arc<tokio::sync::Notify>,
    release: std::sync::Arc<tokio::sync::Notify>,
}

impl Drop for PendingConnection {
    fn drop(&mut self) {
        let Some(connection) = self.0.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ =
                    tokio::time::timeout(Duration::from_secs(5), connection.close_by_ref()).await;
            });
        }
    }
}

/// 迁移期间持有独立连接上的数据库级互斥。
pub(super) struct MigrationLock {
    connection: DatabaseConnection,
    transaction: Option<sea_orm::DatabaseTransaction>,
    released: bool,
}

impl MigrationLock {
    /// 取得迁移锁；竞争时以短间隔重试，超过预算后失败关闭。
    pub(super) async fn acquire(db: &Db, url: &str, wait_timeout: Duration) -> Result<Self> {
        Self::acquire_inner(
            db,
            url,
            wait_timeout,
            #[cfg(test)]
            None,
        )
        .await
    }

    async fn acquire_inner(
        db: &Db,
        url: &str,
        wait_timeout: Duration,
        #[cfg(test)] gate: Option<AcquireGate>,
    ) -> Result<Self> {
        let backend = db.backend()?;
        let primary = db.primary_connection()?;
        let deadline = tokio::time::Instant::now() + wait_timeout;
        let mut options = ConnectOptions::new(url.to_owned());
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(wait_timeout)
            .acquire_timeout(wait_timeout);
        #[cfg(feature = "db-mysql")]
        if backend == DbBackend::MySql {
            options.map_sqlx_mysql_opts(|mysql_options| {
                mysql_options.ssl_mode(sqlx::mysql::MySqlSslMode::Required)
            });
        }
        let connection = tokio::time::timeout_at(deadline, Database::connect(options))
            .await
            .map_err(|_| Error::custom("migration_lock_timeout", "等待数据库迁移锁超时"))?
            .map_err(|_| {
                Error::custom("migration_lock_connection_failed", "无法建立独立迁移锁连接")
            })?;
        let mut pending = PendingConnection(Some(connection.clone()));
        let result = tokio::time::timeout_at(deadline, async {
            Self::verify_same_database(&primary, &connection, backend).await?;
            Self::acquire_on_connection(
                connection.clone(),
                backend,
                #[cfg(test)]
                gate,
            )
            .await
        })
        .await
        .map_err(|_| Error::custom("migration_lock_timeout", "等待数据库迁移锁超时"))?;
        let closed = result.is_ok()
            || matches!(
                tokio::time::timeout(Duration::from_secs(5), connection.close_by_ref()).await,
                Ok(Ok(()))
            );
        if closed {
            pending.0.take();
        }
        result
    }

    async fn verify_same_database(
        primary: &DatabaseConnection,
        lock: &DatabaseConnection,
        backend: DbBackend,
    ) -> Result<()> {
        let query = match backend {
            DbBackend::Postgres => {
                "SELECT current_database() || ':' || (SELECT oid::text FROM pg_database WHERE datname = current_database()) || ':' || EXTRACT(EPOCH FROM pg_postmaster_start_time())::text AS identity"
            }
            DbBackend::MySql => "SELECT CONCAT(DATABASE(), ':', @@server_uuid) AS identity",
            _ => {
                return Err(Error::custom(
                    "unsupported_database_backend",
                    "迁移互斥仅支持 PostgreSQL 和 MySQL",
                ));
            }
        };
        let statement = Statement::from_string(backend, query.to_owned());
        let primary_identity = primary
            .query_one_raw(statement.clone())
            .await
            .map_err(|_| Error::custom("migration_lock_identity_unavailable", "无法核验主库身份"))?
            .ok_or_else(|| {
                Error::custom("migration_lock_identity_unavailable", "无法核验主库身份")
            })?
            .try_get::<String>("", "identity")
            .map_err(|_| {
                Error::custom("migration_lock_identity_unavailable", "无法核验主库身份")
            })?;
        let lock_identity = lock
            .query_one_raw(statement)
            .await
            .map_err(|_| {
                Error::custom(
                    "migration_lock_identity_unavailable",
                    "无法核验迁移锁数据库身份",
                )
            })?
            .ok_or_else(|| {
                Error::custom(
                    "migration_lock_identity_unavailable",
                    "无法核验迁移锁数据库身份",
                )
            })?
            .try_get::<String>("", "identity")
            .map_err(|_| {
                Error::custom(
                    "migration_lock_identity_unavailable",
                    "无法核验迁移锁数据库身份",
                )
            })?;
        if primary_identity != lock_identity {
            return Err(Error::custom(
                "migration_lock_database_mismatch",
                "迁移锁连接与主库不属于同一数据库",
            ));
        }
        Ok(())
    }

    async fn acquire_on_connection(
        connection: DatabaseConnection,
        backend: DbBackend,
        #[cfg(test)] gate: Option<AcquireGate>,
    ) -> Result<Self> {
        let probe = match backend {
            DbBackend::Postgres => "SELECT pg_try_advisory_xact_lock(660123191) AS acquired",
            DbBackend::MySql => {
                "SELECT GET_LOCK(CONCAT('openrust:', MD5(DATABASE())), 0) AS acquired"
            }
            _ => {
                return Err(Error::custom(
                    "unsupported_database_backend",
                    "迁移互斥仅支持 PostgreSQL 和 MySQL",
                ));
            }
        };
        loop {
            let transaction = connection.begin().await.map_err(Error::internal)?;
            let row = transaction
                .query_one_raw(Statement::from_string(backend, probe.to_owned()))
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| Error::custom("migration_lock_unavailable", "迁移互斥未返回结果"))?;
            let acquired = match backend {
                DbBackend::Postgres => row
                    .try_get::<bool>("", "acquired")
                    .map_err(Error::internal)?,
                DbBackend::MySql => {
                    row.try_get::<i64>("", "acquired")
                        .map_err(Error::internal)?
                        == 1
                }
                _ => false,
            };
            if acquired {
                #[cfg(test)]
                if let Some(gate) = gate {
                    gate.entered.notify_one();
                    gate.release.notified().await;
                }
                return Ok(Self {
                    connection,
                    transaction: Some(transaction),
                    released: false,
                });
            }
            transaction.rollback().await.map_err(Error::internal)?;
            tokio::time::sleep(RETRY_INTERVAL).await;
        }
    }

    /// 提交释放锁事务。
    pub(super) async fn release(mut self) -> Result<()> {
        let transaction = self
            .transaction
            .take()
            .ok_or_else(|| Error::custom("migration_lock_release_failed", "迁移互斥已经释放"))?;
        let unlocked = if self.connection.get_database_backend() == DbBackend::MySql {
            let row = transaction
                .query_one_raw(Statement::from_string(
                    DbBackend::MySql,
                    "SELECT RELEASE_LOCK(CONCAT('openrust:', MD5(DATABASE()))) AS released"
                        .to_owned(),
                ))
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| {
                    Error::custom("migration_lock_release_failed", "迁移互斥释放未返回结果")
                })?;
            if row
                .try_get::<i64>("", "released")
                .map_err(Error::internal)?
                == 1
            {
                Ok(())
            } else {
                Err(Error::custom(
                    "migration_lock_release_failed",
                    "迁移互斥释放失败",
                ))
            }
        } else {
            Ok(())
        };
        let committed = transaction.commit().await.map_err(Error::internal);
        let closed = self
            .connection
            .close_by_ref()
            .await
            .map_err(Error::internal);
        unlocked?;
        committed?;
        closed?;
        self.released = true;
        Ok(())
    }
}

impl Drop for MigrationLock {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // 事务析构回滚；存活 runtime 中显式关闭独立池，避免 MySQL 会话锁滞留。
        self.transaction.take();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let connection = self.connection.clone();
            runtime.spawn(async move {
                let _ =
                    tokio::time::timeout(Duration::from_secs(5), connection.close_by_ref()).await;
            });
        }
    }
}

pub(super) const fn default_wait_timeout() -> Duration {
    DEFAULT_WAIT_TIMEOUT
}

#[cfg(all(test, feature = "db-mysql"))]
mod tests {
    use super::*;
    use sea_orm_migration::MigratorTrait;

    struct EmptyMigrator;

    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
            Vec::new()
        }
    }

    #[tokio::test]
    async fn cancelled_acquire_closes_mysql_session_before_guard_exists()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping MySQL lock cancellation: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        if !url.starts_with("mysql://") {
            return Ok(());
        }
        let isolated = crate::testing::TestDatabase::create::<EmptyMigrator>(&url).await?;
        let mut options = ConnectOptions::new(isolated.url().to_owned());
        options.max_connections(1).min_connections(1);
        options.map_sqlx_mysql_opts(|mysql| mysql.ssl_mode(sqlx::mysql::MySqlSslMode::Required));
        let primary = Database::connect(options).await?;
        let db = Db::default();
        db.initialize(primary.clone())?;
        db.configure_migration_lock_url(isolated.url().to_owned())?;
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let observed = entered.clone();
        let lock_url = isolated.url().to_owned();
        let worker = tokio::spawn(async move {
            MigrationLock::acquire_inner(
                &db,
                &lock_url,
                Duration::from_secs(5),
                Some(AcquireGate { entered, release }),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), observed.notified()).await?;
        worker.abort();
        let _ = worker.await;

        let contender = Database::connect(isolated.url()).await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let row = contender
                    .query_one_raw(Statement::from_string(
                        DbBackend::MySql,
                        "SELECT GET_LOCK(CONCAT('openrust:', MD5(DATABASE())), 0) AS acquired"
                            .to_owned(),
                    ))
                    .await?
                    .ok_or("contending GET_LOCK returned no row")?;
                if row.try_get::<i64>("", "acquired")? == 1 {
                    break Ok::<(), Box<dyn std::error::Error>>(());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        contender
            .execute_unprepared("SELECT RELEASE_LOCK(CONCAT('openrust:', MD5(DATABASE())))")
            .await?;
        contender.close_by_ref().await?;
        primary.close_by_ref().await?;
        isolated.close().await?;
        Ok(())
    }

    #[tokio::test]
    async fn identity_query_wait_obeys_absolute_acquire_deadline()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping lock identity deadline: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        if !url.starts_with("mysql://") {
            return Ok(());
        }
        let isolated = crate::testing::TestDatabase::create::<EmptyMigrator>(&url).await?;
        let mut options = ConnectOptions::new(isolated.url().to_owned());
        options.max_connections(1).min_connections(1);
        options.map_sqlx_mysql_opts(|mysql| mysql.ssl_mode(sqlx::mysql::MySqlSslMode::Required));
        let primary = Database::connect(options).await?;
        let db = Db::default();
        db.initialize(primary.clone())?;
        db.configure_migration_lock_url(isolated.url().to_owned())?;
        let held = primary.begin().await?;
        let result = MigrationLock::acquire(&db, isolated.url(), Duration::from_millis(500)).await;
        assert_eq!(
            result.err().map(|error| error.code().to_owned()),
            Some("migration_lock_timeout".to_owned())
        );
        held.rollback().await?;
        let lock = MigrationLock::acquire(&db, isolated.url(), Duration::from_secs(5)).await?;
        lock.release().await?;
        primary.close_by_ref().await?;
        isolated.close().await?;
        Ok(())
    }
}
