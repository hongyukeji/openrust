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

pub mod dialect;
mod migration;
mod rate_limit;
mod seed;
mod table_names;

pub use crate::config::database::DatabaseConfig;
pub use migration::MigrationPlugin;
pub use rate_limit::{DatabaseRateLimitMigration, DatabaseRateLimitStore};
pub use sea_orm;
pub use sea_orm::{DatabaseConnection, DatabaseTransaction, DbBackend};
pub use sea_orm_migration;
pub use seed::SeedPlugin;
pub use table_names::DatabaseTableNames;

/// A database backend included in OpenRust's official PostgreSQL/MySQL feature set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    /// PostgreSQL 16 or newer; daily development and CI target PostgreSQL 18.
    PostgreSql,
    /// MySQL 8.4 or newer, the current broad-adoption LTS development target.
    MySql,
}

impl DatabaseBackend {
    /// Identifies one of the official backends from its connection URL scheme.
    ///
    /// # Errors
    ///
    /// Returns a configuration error for URL schemes outside the official backend set.
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

    /// Returns whether this backend's Cargo feature is enabled in the `openrust` package.
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::PostgreSql => cfg!(feature = "db-postgres"),
            Self::MySql => cfg!(feature = "db-mysql"),
        }
    }

    /// Cargo feature required to compile this backend.
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

/// Cloneable handle to the database connection pool registered by [`DatabasePlugin`].
#[derive(Clone, Default)]
pub struct Db {
    connections: Arc<OnceLock<DatabaseConnections>>,
    table_names: Arc<DatabaseTableNames>,
    next_replica: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct DatabaseConnections {
    primary: DatabaseConnection,
    read_replicas: Vec<DatabaseConnection>,
}

impl Db {
    /// Installs a SeaORM connection supplied by a custom database plugin.
    ///
    /// Custom plugins can register `Db::default()` during `build`, then connect and call this
    /// method during `finish`. The connection must use a backend supported by SeaORM. This keeps
    /// transactions, migrations, and seeds on the same framework service; custom plugins remain
    /// responsible for validating capabilities their application requires.
    ///
    /// # Errors
    ///
    /// Returns an error when this handle has already been initialized.
    pub fn initialize(&self, connection: DatabaseConnection) -> Result<()> {
        self.initialize_with_replicas(connection, Vec::new())
    }

    /// Installs the primary and optional read-only replica pools for a custom database plugin.
    ///
    /// Every pool must use the same backend. The framework does not configure or verify database
    /// replication; callers must ensure replicas are read-only and accept replication lag.
    ///
    /// # Errors
    ///
    /// Returns an error if a pool has a different backend or this handle was already initialized.
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

    /// Creates a database handle using the supplied immutable table-name resolver.
    pub fn with_table_names(table_names: DatabaseTableNames) -> Self {
        Self {
            connections: Arc::new(OnceLock::new()),
            table_names: Arc::new(table_names),
            next_replica: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Returns the configured physical table name for a logical name.
    pub fn table_name(&self, logical_name: &str) -> Result<String> {
        let backend = self.backend()?;
        self.table_names.resolve(logical_name).and_then(|name| {
            self.table_names.quote(logical_name, backend)?;
            Ok(name)
        })
    }

    /// Returns the configured and backend-quoted physical table name.
    pub fn quoted_table_name(&self, logical_name: &str) -> Result<String> {
        self.table_names.quote(logical_name, self.backend()?)
    }

    /// Clones the immutable resolver used by this database handle.
    pub fn table_names(&self) -> DatabaseTableNames {
        self.table_names.as_ref().clone()
    }

    /// Returns the initialized SeaORM backend for this connection.
    ///
    /// # Errors
    ///
    /// Returns an internal error if the pool is not initialized.
    pub fn backend(&self) -> Result<DbBackend> {
        Ok(self.primary_connection()?.get_database_backend())
    }

    /// Returns a clone of the initialized SeaORM connection pool.
    ///
    /// # Errors
    ///
    /// Returns an internal error if the database plugin has not finished initializing.
    pub fn connection(&self) -> Result<DatabaseConnection> {
        self.primary_connection()
    }

    /// Returns a clone of the primary SeaORM connection pool.
    pub fn primary_connection(&self) -> Result<DatabaseConnection> {
        self.connections
            .get()
            .map(|connections| connections.primary.clone())
            .ok_or_else(|| Error::internal(DatabaseNotReady))
    }

    /// Returns a read connection selected by round robin, or the primary when no replicas exist.
    ///
    /// The caller must perform only read-only work that can tolerate replication lag. This method
    /// does not inspect SQL and does not retry or fall back after a query error. Within
    /// [`with_primary`](Self::with_primary), it always returns the primary.
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

    /// Runs a future in a task-local scope where [`read_connection`](Self::read_connection)
    /// returns this handle's primary connection.
    ///
    /// The scope does not propagate to independently spawned Tokio tasks. Nesting scopes for the
    /// same handle restores the outer scope when the inner future completes.
    pub async fn with_primary<F: std::future::Future>(&self, future: F) -> Result<F::Output> {
        let primary = self.primary_connection()?;
        Ok(PRIMARY_CONNECTION_SCOPE
            .scope((Arc::as_ptr(&self.connections) as usize, primary), future)
            .await)
    }

    /// Runs an operation in a database transaction.
    ///
    /// The callback's error rolls back the transaction. A successful callback commits it.
    ///
    /// # Errors
    ///
    /// Returns an internal error if the pool is not initialized, transaction setup or completion
    /// fails, or the callback returns an error.
    pub async fn transaction<T, F>(&self, callback: F) -> Result<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
    {
        self.connection()?
            .transaction(callback)
            .await
            .map_err(Error::internal)
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

/// Installs the configured PostgreSQL or MySQL connection pool as the [`Db`] application service.
///
/// Enable `db-postgres` or `db-mysql` on `openrust` to include a driver. The application must
/// provide a URL under `[database].url`, `DATABASE_URL`, or `APP_DATABASE__URL`.
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
            DatabaseBackend::PostgreSql => "16",
            DatabaseBackend::MySql => "8.4",
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
            .is_ok_and(|server_version| server_version >= 160_000),
        DatabaseBackend::MySql => {
            if version.to_ascii_lowercase().contains("mariadb") {
                return false;
            }
            let mut parts = version.split('.');
            let major = parts.next().and_then(|part| part.parse::<u32>().ok());
            let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
            matches!((major, minor), (Some(major), Some(minor)) if (major, minor) >= (8, 4))
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
    fn broad_adoption_database_development_targets_are_checked_explicitly() {
        assert!(is_server_version_supported(
            "160000",
            DatabaseBackend::PostgreSql
        ));
        assert!(!is_server_version_supported(
            "150019",
            DatabaseBackend::PostgreSql
        ));
        assert!(is_server_version_supported(
            "8.4.13",
            DatabaseBackend::MySql
        ));
        assert!(!is_server_version_supported(
            "8.0.44",
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
