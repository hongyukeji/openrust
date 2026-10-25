#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::config::ConfigError;
use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, RateLimiter, Result};
use sea_orm::ConnectionTrait;
use sea_orm::{ConnectOptions, Database, Statement, TransactionTrait};
use serde::Deserialize;
use std::sync::OnceLock;

pub mod dialect;
mod migration;
mod rate_limit;
mod seed;
mod table_names;

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
    /// PostgreSQL 16 or newer, the current broad-adoption development target.
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

/// Database connection and pool settings from the `[database]` configuration section.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Database connection URL. `DATABASE_URL` and `APP_DATABASE__URL` map here.
    pub url: Option<String>,
    /// Maximum number of open pooled connections.
    pub max_connections: u32,
    /// Minimum number of open pooled connections.
    pub min_connections: u32,
    /// Maximum time to establish a connection, in seconds.
    pub connect_timeout_secs: u64,
    /// Maximum time to wait for an available pooled connection, in seconds.
    pub acquire_timeout_secs: u64,
    /// Prefix applied to logical framework table names unless a full-name mapping overrides it.
    pub table_prefix: String,
    /// Maps logical table names to complete physical names, without applying `table_prefix`.
    pub table_names: std::collections::BTreeMap<String, String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: None,
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 30,
            table_prefix: "tb_".to_owned(),
            table_names: std::collections::BTreeMap::new(),
        }
    }
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("url", &self.url.as_ref().map(|_| "[REDACTED]"))
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("acquire_timeout_secs", &self.acquire_timeout_secs)
            .field("table_prefix", &self.table_prefix)
            .field("table_names", &self.table_names)
            .finish()
    }
}

/// Cloneable handle to the database connection pool registered by [`DatabasePlugin`].
#[derive(Clone, Default)]
pub struct Db {
    connection: Arc<OnceLock<DatabaseConnection>>,
    table_names: Arc<DatabaseTableNames>,
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
        self.table_names
            .validate_backend(connection.get_database_backend())?;
        self.connection.set(connection).map_err(|_| {
            Error::custom(
                "database_already_initialized",
                "database connection pool was initialized more than once",
            )
        })
    }

    /// Creates a database handle using the supplied immutable table-name resolver.
    pub fn with_table_names(table_names: DatabaseTableNames) -> Self {
        Self {
            connection: Arc::new(OnceLock::new()),
            table_names: Arc::new(table_names),
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
        Ok(self.connection()?.get_database_backend())
    }

    /// Returns a clone of the initialized SeaORM connection pool.
    ///
    /// # Errors
    ///
    /// Returns an internal error if the database plugin has not finished initializing.
    pub fn connection(&self) -> Result<DatabaseConnection> {
        self.connection
            .get()
            .cloned()
            .ok_or_else(|| Error::internal(DatabaseNotReady))
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

impl fmt::Debug for Db {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Db")
            .field("initialized", &self.connection.get().is_some())
            .field("table_names", &self.table_names)
            .finish()
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
        let config = app.section::<DatabaseConfig>("database")?;
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
            let mut options = ConnectOptions::new(url.to_owned());
            options
                .max_connections(config.max_connections)
                .min_connections(config.min_connections)
                .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
                .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs));
            let backend = DatabaseBackend::from_url(url)?;
            #[cfg(feature = "db-mysql")]
            if backend == DatabaseBackend::MySql {
                options.map_sqlx_mysql_opts(|mysql_options| {
                    mysql_options.ssl_mode(sqlx::mysql::MySqlSslMode::Required)
                });
            }
            if backend == DatabaseBackend::MySql {
                #[cfg(feature = "db-mysql")]
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
                    "could not connect to configured {} database",
                    backend.name()
                )))
            })?;
            validate_server_version(&connection, backend).await?;
            db.initialize(connection)?;
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

    Ok(())
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
