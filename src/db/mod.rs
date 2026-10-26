#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::config::ConfigError;
use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, RateLimiter, Result};
use sea_orm::ConnectionTrait;
use sea_orm::{ConnectOptions, Database, Statement, TransactionTrait};
use std::sync::OnceLock;

tokio::task_local! {
    static PRIMARY_CONNECTION_SCOPE: (usize, DatabaseConnection);
}

mod coordination;
pub mod dialect;
mod migration;
mod rate_limit;
mod seed;
mod table_names;

pub use crate::config::database::DatabaseConfig;
pub use coordination::{
    CoordinationMigration, IdempotencyClaim, IdempotencyLease, IdempotencyResponse, LockAcquire,
    LockLease, RequestIdempotency, SharedLocks,
};
pub use migration::MigrationPlugin;
pub use rate_limit::{DatabaseRateLimitMigration, DatabaseRateLimitStore};
pub use sea_orm;
pub use sea_orm::{DatabaseConnection, DatabaseTransaction, DbBackend};
pub use sea_orm_migration;
pub use seed::SeedPlugin;
pub use table_names::DatabaseTableNames;

/// OpenRust 官方 PostgreSQL/MySQL feature 集支持的数据库后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    /// PostgreSQL 14 或更高版本；日常开发与 CI 使用 PostgreSQL 18。
    PostgreSql,
    /// MySQL 8.0 或更高版本；日常开发与 CI 使用 MySQL 8.4 LTS。
    MySql,
}

impl DatabaseBackend {
    /// 根据连接 URL scheme 识别一种官方支持的后端。
    ///
    /// # Errors
    ///
    /// URL scheme 不属于官方支持的后端时，返回配置错误。
    pub fn from_url(url: &str) -> Result<Self> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Ok(Self::PostgreSql)
        } else if url.starts_with("mysql://") {
            Ok(Self::MySql)
        } else {
            Err(Error::custom(
                "unsupported_database_backend",
                "OpenRust officially supports PostgreSQL and MySQL database URLs",
            ))
        }
    }

    /// 返回 `openrust` package 是否启用了此后端对应的 Cargo feature。
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::PostgreSql => cfg!(feature = "db-postgres"),
            Self::MySql => cfg!(feature = "db-mysql"),
        }
    }

    /// 编译此后端所需的 Cargo feature。
    pub const fn feature(self) -> &'static str {
        match self {
            Self::PostgreSql => "db-postgres",
            Self::MySql => "db-mysql",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::PostgreSql => "PostgreSQL",
            Self::MySql => "MySQL",
        }
    }
}

/// 对 [`DatabasePlugin`] 注册的数据库连接池的可克隆句柄。
#[derive(Clone, Default)]
pub struct Db {
    connections: Arc<OnceLock<DatabaseConnections>>,
    table_names: Arc<DatabaseTableNames>,
    next_replica: Arc<AtomicUsize>,
    events: Arc<OnceLock<crate::events::Events>>,
}

/// 派发给事务监听器、且仍可回滚的提交前边界。
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionBeforeCommit;

/// 仅在事务成功提交后派发的进程内通知。
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionCommitted;

#[derive(Clone)]
struct DatabaseConnections {
    primary: DatabaseConnection,
    read_replicas: Vec<DatabaseConnection>,
}

impl Db {
    /// 安装自定义数据库插件提供的 SeaORM 连接。
    ///
    /// 自定义插件可在 `build` 阶段注册 `Db::default()`，随后建立连接并在 `finish` 阶段调用此方法。
    /// 连接必须使用 SeaORM 支持的后端。这样事务、迁移和种子操作都会使用同一个框架服务；自定义插件仍须
    /// 验证应用所需的能力。
    ///
    /// # Errors
    ///
    /// 此句柄已初始化时返回错误。
    pub fn initialize(&self, connection: DatabaseConnection) -> Result<()> {
        self.initialize_with_replicas(connection, Vec::new())
    }

    /// 为自定义数据库插件安装主库连接池和可选的只读副本连接池。
    ///
    /// 所有连接池必须使用相同后端。框架不会配置或验证数据库复制；调用方必须确保副本为只读，并接受复制延迟。
    ///
    /// # Errors
    ///
    /// 任一连接池使用了不同后端，或此句柄已初始化时返回错误。
    pub fn initialize_with_replicas(
        &self,
        primary: DatabaseConnection,
        read_replicas: Vec<DatabaseConnection>,
    ) -> Result<()> {
        let backend = primary.get_database_backend();
        self.table_names.validate_backend(backend)?;
        if read_replicas
            .iter()
            .any(|replica| replica.get_database_backend() != backend)
        {
            return Err(Error::custom(
                "database_replica_backend_mismatch",
                "all database read replicas must use the same backend as the primary",
            ));
        }
        self.connections
            .set(DatabaseConnections {
                primary,
                read_replicas,
            })
            .map_err(|_| {
                Error::custom(
                    "database_already_initialized",
                    "database connection pools were initialized more than once",
                )
            })
    }

    /// 使用传入的不可变表名解析器创建数据库句柄。
    pub fn with_table_names(table_names: DatabaseTableNames) -> Self {
        Self {
            connections: Arc::new(OnceLock::new()),
            table_names: Arc::new(table_names),
            next_replica: Arc::new(AtomicUsize::new(0)),
            events: Arc::new(OnceLock::new()),
        }
    }

    fn install_events(&self, events: crate::events::Events) {
        let _ = self.events.set(events);
    }

    #[cfg(feature = "auth")]
    pub(crate) fn events(&self) -> Option<crate::events::Events> {
        self.events.get().cloned()
    }

    /// 返回逻辑表名对应的已配置物理表名。
    pub fn table_name(&self, logical_name: &str) -> Result<String> {
        let backend = self.backend()?;
        self.table_names.resolve(logical_name).and_then(|name| {
            self.table_names.quote(logical_name, backend)?;
            Ok(name)
        })
    }

    /// 返回经过后端引用符处理的已配置物理表名。
    pub fn quoted_table_name(&self, logical_name: &str) -> Result<String> {
        self.table_names.quote(logical_name, self.backend()?)
    }

    /// 克隆此数据库句柄使用的不可变解析器。
    pub fn table_names(&self) -> DatabaseTableNames {
        self.table_names.as_ref().clone()
    }

    /// 返回此连接已初始化的 SeaORM 后端。
    ///
    /// # Errors
    ///
    /// 连接池尚未初始化时返回内部错误。
    pub fn backend(&self) -> Result<DbBackend> {
        Ok(self.primary_connection()?.get_database_backend())
    }

    /// 返回已初始化 SeaORM 连接池的克隆。
    ///
    /// # Errors
    ///
    /// 数据库插件尚未完成初始化时返回内部错误。
    pub fn connection(&self) -> Result<DatabaseConnection> {
        self.primary_connection()
    }

    /// 返回主库 SeaORM 连接池的克隆。
    pub fn primary_connection(&self) -> Result<DatabaseConnection> {
        self.connections
            .get()
            .map(|connections| connections.primary.clone())
            .ok_or_else(|| Error::internal(DatabaseNotReady))
    }

    /// 以轮询方式返回一个读连接；没有副本时返回主库连接。
    ///
    /// 调用方只能在此连接上执行可接受复制延迟的只读操作。此方法不会检查 SQL，也不会在查询出错后重试或回退。
    /// 在 [`with_primary`](Self::with_primary) 作用域中，此方法始终返回主库连接。
    pub fn read_connection(&self) -> Result<DatabaseConnection> {
        let connections = self
            .connections
            .get()
            .ok_or_else(|| Error::internal(DatabaseNotReady))?;
        let handle_id = Arc::as_ptr(&self.connections) as usize;
        if let Ok(Some(connection)) = PRIMARY_CONNECTION_SCOPE
            .try_with(|(scope_id, connection)| (*scope_id == handle_id).then(|| connection.clone()))
        {
            return Ok(connection);
        }
        if connections.read_replicas.is_empty() {
            return Ok(connections.primary.clone());
        }
        let index = next_replica_index(connections.read_replicas.len(), &self.next_replica);
        Ok(connections.read_replicas[index].clone())
    }

    /// 在任务本地作用域内运行 future；作用域中的 [`read_connection`](Self::read_connection) 会返回此句柄的主库连接。
    ///
    /// 此作用域不会传播到独立创建的 Tokio task。为同一句柄嵌套作用域时，内层 future 完成后会恢复外层作用域。
    pub async fn with_primary<F: std::future::Future>(&self, future: F) -> Result<F::Output> {
        let primary = self.primary_connection()?;
        Ok(PRIMARY_CONNECTION_SCOPE
            .scope((Arc::as_ptr(&self.connections) as usize, primary), future)
            .await)
    }

    /// 在数据库事务中运行操作。
    ///
    /// 回调成功后，在标准 [`DatabasePlugin`] 与 `EventsPlugin` 同时安装时会先派发
    /// [`TransactionBeforeCommit`]；该事件任一监听器失败都会回滚事务。提交成功后尽力派发
    /// [`TransactionCommitted`] 的 direct listeners；其错误只记录稳定错误码，不改变已提交结果。
    /// 可靠的提交后副作用应在提交前事件中排入同一事务的队列。
    ///
    /// # Errors
    ///
    /// 连接池尚未初始化、事务开始或提交失败，或回调/提交前监听器返回错误时，返回内部错误。
    pub async fn transaction<T, F>(&self, callback: F) -> Result<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
    {
        let events = self.events.get().cloned();
        let transaction = self.connection()?.begin().await.map_err(Error::internal)?;
        let value = match callback(&transaction).await {
            Ok(value) => value,
            Err(error) => {
                transaction.rollback().await.map_err(Error::internal)?;
                return Err(Error::internal(error));
            }
        };
        if let Some(events) = &events
            && let Err(error) = events
                .dispatch_in_transaction(&transaction, &TransactionBeforeCommit)
                .await
        {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(Error::internal(error));
        }
        transaction.commit().await.map_err(Error::internal)?;
        if let Some(events) = self.events.get()
            && let Err(error) = events.dispatch_direct(&TransactionCommitted).await
        {
            tracing::error!(
                error_code = error.code(),
                "事务已提交，但提交后监听器执行失败"
            );
        }
        Ok(value)
    }
}

fn next_replica_index(replica_count: usize, counter: &AtomicUsize) -> usize {
    counter.fetch_add(1, Ordering::Relaxed) % replica_count
}

impl fmt::Debug for Db {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Db")
            .field("initialized", &self.connections.get().is_some())
            .field("table_names", &self.table_names)
            .finish_non_exhaustive()
    }
}

/// 将已配置的 PostgreSQL 或 MySQL 连接池安装为 [`Db`] 应用服务。
///
/// 在 `openrust` 上启用 `db-postgres` 或 `db-mysql` 以包含相应驱动。应用必须通过 `[database].url`、
/// `DATABASE_URL` 或 `APP_DATABASE__URL` 提供 URL。
#[derive(Debug, Default, Clone, Copy)]
pub struct DatabasePlugin;

impl Plugin for DatabasePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        if app.get::<Db>().is_some() {
            return Err(Error::custom(
                "database_service_already_registered",
                "register only one database plugin per application",
            ));
        }
        let config = app.sensitive_section::<DatabaseConfig>("database")?;
        validate_config(&config)?;
        let table_names =
            DatabaseTableNames::new(config.table_prefix.clone(), config.table_names.clone())?;
        app.insert(config);
        let db = Db::with_table_names(table_names);
        app.insert(RateLimiter::new(DatabaseRateLimitStore::new(db.clone())));
        app.insert(db);
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if let (Some(db), Some(events)) = (
                app.state().get::<Db>(),
                app.state().get::<crate::events::Events>(),
            ) {
                db.install_events((*events).clone());
            }
            let config = app.state().try_get::<DatabaseConfig>()?;
            let db = app.state().try_get::<Db>()?;
            let url = config.url.as_deref().ok_or_else(|| {
                Error::Config(ConfigError::Invalid(
                    "missing database.url (set DATABASE_URL or APP_DATABASE__URL)".into(),
                ))
            })?;
            let backend = DatabaseBackend::from_url(url)?;
            let connection = connect_database(url, &config, backend, "primary").await?;
            let mut replicas = Vec::with_capacity(config.read_replicas.len());
            for (index, replica_url) in config.read_replicas.iter().enumerate() {
                let replica_backend = DatabaseBackend::from_url(replica_url)?;
                if replica_backend != backend {
                    return Err(Error::Config(ConfigError::Invalid(format!(
                        "database.read_replicas[{index}] must use the same backend as database.url"
                    ))));
                }
                replicas.push(
                    connect_database(replica_url, &config, replica_backend, "read replica").await?,
                );
            }
            db.initialize_with_replicas(connection, replicas)?;
            Ok(())
        })
    }
}

fn validate_config(config: &DatabaseConfig) -> Result<()> {
    DatabaseTableNames::new(config.table_prefix.clone(), config.table_names.clone())?;
    if config.max_connections == 0
        || config.min_connections > config.max_connections
        || config.connect_timeout_secs == 0
        || config.acquire_timeout_secs == 0
    {
        return Err(Error::Config(ConfigError::Invalid(
            "database pool sizes and timeouts must be positive, and min_connections must not exceed max_connections".into(),
        )));
    }

    if let Some(url) = config.url.as_deref() {
        let backend = DatabaseBackend::from_url(url)?;
        if !backend.is_enabled() {
            return Err(Error::Config(ConfigError::Invalid(format!(
                "{} database URL requires the openrust feature `{}`",
                backend.name(),
                backend.feature()
            ))));
        }
    }

    for (index, url) in config.read_replicas.iter().enumerate() {
        let backend = DatabaseBackend::from_url(url).map_err(|_| {
            Error::Config(ConfigError::Invalid(format!(
                "database.read_replicas[{index}] has an unsupported database URL"
            )))
        })?;
        if !backend.is_enabled() {
            return Err(Error::Config(ConfigError::Invalid(format!(
                "database.read_replicas[{index}] requires the openrust feature `{}`",
                backend.feature()
            ))));
        }
        if config
            .url
            .as_deref()
            .and_then(|primary| DatabaseBackend::from_url(primary).ok())
            .is_some_and(|primary| primary != backend)
        {
            return Err(Error::Config(ConfigError::Invalid(format!(
                "database.read_replicas[{index}] must use the same backend as database.url"
            ))));
        }
    }

    Ok(())
}

async fn connect_database(
    url: &str,
    config: &DatabaseConfig,
    backend: DatabaseBackend,
    role: &str,
) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_owned());
    options
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs));
    #[cfg(feature = "db-mysql")]
    if backend == DatabaseBackend::MySql {
        options.map_sqlx_mysql_opts(|mysql_options| {
            mysql_options.ssl_mode(sqlx::mysql::MySqlSslMode::Required)
        });
    }
    #[cfg(feature = "db-mysql")]
    if backend == DatabaseBackend::MySql {
        options.after_connect(|connection| {
            Box::pin(async move {
                connection
                    .execute_unprepared("SET time_zone = '+00:00'")
                    .await?;
                Ok(())
            })
        });
    }
    let connection = Database::connect(options).await.map_err(|_| {
        Error::Config(ConfigError::Invalid(format!(
            "could not connect to configured {} {role}",
            backend.name()
        )))
    })?;
    validate_server_version(&connection, backend).await?;
    Ok(connection)
}

async fn validate_server_version(
    connection: &sea_orm::DatabaseConnection,
    backend: DatabaseBackend,
) -> Result<()> {
    let (sea_backend, query) = match backend {
        DatabaseBackend::PostgreSql => (
            DbBackend::Postgres,
            "SELECT current_setting('server_version_num') AS version",
        ),
        DatabaseBackend::MySql => (DbBackend::MySql, "SELECT VERSION() AS version"),
    };
    let version = connection
        .query_one_raw(Statement::from_string(sea_backend, query.to_owned()))
        .await
        .map_err(|_| {
            Error::Config(ConfigError::Invalid(format!(
                "could not verify configured {} database version",
                backend.name()
            )))
        })?
        .and_then(|row| row.try_get::<String>("", "version").ok())
        .ok_or_else(|| {
            Error::Config(ConfigError::Invalid(format!(
                "could not verify configured {} database version",
                backend.name()
            )))
        })?;
    if !is_server_version_supported(&version, backend) {
        let minimum = match backend {
            DatabaseBackend::PostgreSql => "14",
            DatabaseBackend::MySql => "8.0",
        };
        return Err(Error::Config(ConfigError::Invalid(format!(
            "configured {} database version is unsupported; OpenRust requires {minimum} or newer",
            backend.name()
        ))));
    }
    Ok(())
}

fn is_server_version_supported(version: &str, backend: DatabaseBackend) -> bool {
    match backend {
        DatabaseBackend::PostgreSql => version
            .parse::<u32>()
            .is_ok_and(|server_version| server_version >= 140_000),
        DatabaseBackend::MySql => {
            if version.to_ascii_lowercase().contains("mariadb") {
                return false;
            }
            let mut parts = version.split('.');
            let major = parts.next().and_then(|part| part.parse::<u32>().ok());
            let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
            matches!((major, minor), (Some(major), Some(minor)) if (major, minor) >= (8, 0))
        }
    }
}

#[derive(Debug)]
struct DatabaseNotReady;

impl std::fmt::Display for DatabaseNotReady {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("database plugin has not initialized the connection pool")
    }
}

impl std::error::Error for DatabaseNotReady {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_define_a_small_pool_without_a_connection_url() {
        let config = DatabaseConfig::default();
        assert_eq!(config.url, None);
        assert_eq!(config.read_replicas, Vec::<String>::new());
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 1);
        assert_eq!(config.connect_timeout_secs, 5);
        assert_eq!(config.acquire_timeout_secs, 30);
    }

    #[test]
    fn debug_redacts_connection_url() {
        let config = DatabaseConfig {
            url: Some("postgres://user:secret@localhost/db".into()),
            ..DatabaseConfig::default()
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("secret"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn debug_does_not_render_replica_urls() {
        let config = DatabaseConfig {
            read_replicas: vec!["postgres://replica:secret@host/db".into()],
            ..DatabaseConfig::default()
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("replica@"));
        assert!(rendered.contains("1 configured"));
    }

    #[test]
    fn replica_selection_round_robins_without_skipping_configured_entries() {
        let counter = AtomicUsize::new(0);
        let selected = (0..5)
            .map(|_| next_replica_index(3, &counter))
            .collect::<Vec<_>>();
        assert_eq!(selected, [0, 1, 2, 0, 1]);
    }

    #[test]
    fn connection_is_unavailable_before_plugin_initialization() {
        assert!(Db::default().connection().is_err());
    }

    #[test]
    fn invalid_pool_settings_are_rejected() {
        let config = DatabaseConfig {
            min_connections: 11,
            ..DatabaseConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn database_url_requires_its_backend_feature() {
        let config = DatabaseConfig {
            url: Some("postgres://user:secret@localhost/db".into()),
            ..DatabaseConfig::default()
        };
        assert_eq!(
            validate_config(&config).is_ok(),
            cfg!(feature = "db-postgres")
        );
    }

    #[test]
    fn replica_backend_requires_its_feature_and_matches_primary() {
        let postgres = DatabaseConfig {
            url: Some("postgres://primary@localhost/db".into()),
            read_replicas: vec!["mysql://replica@localhost/db".into()],
            ..DatabaseConfig::default()
        };
        assert!(validate_config(&postgres).is_err());

        let missing_feature = DatabaseConfig {
            url: Some("postgres://primary@localhost/db".into()),
            read_replicas: vec!["postgres://replica@localhost/db".into()],
            ..DatabaseConfig::default()
        };
        assert_eq!(
            validate_config(&missing_feature).is_ok(),
            cfg!(feature = "db-postgres")
        );
    }

    #[test]
    fn official_backend_detection_rejects_unknown_url_schemes() {
        assert!(matches!(
            DatabaseBackend::from_url("postgresql://user@host/db"),
            Ok(DatabaseBackend::PostgreSql)
        ));
        assert!(matches!(
            DatabaseBackend::from_url("mysql://user@host/db"),
            Ok(DatabaseBackend::MySql)
        ));
        assert!(DatabaseBackend::from_url("sqlite::memory:").is_err());
    }

    #[test]
    fn provisional_minimum_database_versions_are_checked_explicitly() {
        assert!(is_server_version_supported(
            "140000",
            DatabaseBackend::PostgreSql
        ));
        assert!(is_server_version_supported(
            "160000",
            DatabaseBackend::PostgreSql
        ));
        assert!(!is_server_version_supported(
            "130022",
            DatabaseBackend::PostgreSql
        ));
        assert!(is_server_version_supported(
            "8.0.44",
            DatabaseBackend::MySql
        ));
        assert!(is_server_version_supported(
            "8.4.13",
            DatabaseBackend::MySql
        ));
        assert!(!is_server_version_supported(
            "5.7.44",
            DatabaseBackend::MySql
        ));
        assert!(!is_server_version_supported(
            "10.6.18-MariaDB",
            DatabaseBackend::MySql
        ));
        assert!(!is_server_version_supported(
            "unknown",
            DatabaseBackend::MySql
        ));
    }
}
