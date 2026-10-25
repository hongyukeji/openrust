#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use openrust_core::{App, AppBuilder, BoxFuture, Error, Plugin, Result, config::ConfigError};
#[cfg(feature = "db-mysql")]
use sea_orm::ConnectionTrait;
use sea_orm::{
    ConnectOptions, Database, DatabaseConnection, DatabaseTransaction, TransactionTrait,
};
use serde::Deserialize;
use std::sync::OnceLock;

mod migration;

pub use migration::MigrationPlugin;
pub use sea_orm_migration;

/// Database connection and pool settings from the `[database]` configuration section.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Database connection URL. `DATABASE_URL` and `OPENRUST_DATABASE__URL` map here.
    pub url: Option<String>,
    /// Maximum number of open pooled connections.
    pub max_connections: u32,
    /// Minimum number of open pooled connections.
    pub min_connections: u32,
    /// Maximum time to establish a connection, in seconds.
    pub connect_timeout_secs: u64,
    /// Maximum time to wait for an available pooled connection, in seconds.
    pub acquire_timeout_secs: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: None,
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 30,
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
            .finish()
    }
}

/// Cloneable handle to the database connection pool registered by [`DatabasePlugin`].
#[derive(Clone, Default)]
pub struct Db {
    connection: Arc<OnceLock<DatabaseConnection>>,
}

impl Db {
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
            .finish()
    }
}

/// Installs the configured PostgreSQL or MySQL connection pool as the [`Db`] application service.
///
/// Enable `db-postgres` or `db-mysql` on `openrust-db` to include a driver. The application must
/// provide a URL under `[database].url`, `DATABASE_URL`, or `OPENRUST_DATABASE__URL`.
#[derive(Debug, Default, Clone, Copy)]
pub struct DatabasePlugin;

impl Plugin for DatabasePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = app.section::<DatabaseConfig>("database")?;
        validate_config(&config)?;
        app.insert(config);
        app.insert(Db::default());
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> openrust_core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.state().try_get::<DatabaseConfig>()?;
            let db = app.state().try_get::<Db>()?;
            let url = config.url.as_deref().ok_or_else(|| {
                Error::Config(ConfigError::Invalid(
                    "missing database.url (set DATABASE_URL or OPENRUST_DATABASE__URL)".into(),
                ))
            })?;
            let mut options = ConnectOptions::new(url.to_owned());
            options
                .max_connections(config.max_connections)
                .min_connections(config.min_connections)
                .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
                .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs));
            #[cfg(feature = "db-mysql")]
            if backend_name(url) == "MySQL" {
                options.map_sqlx_mysql_opts(|mysql_options| {
                    mysql_options.ssl_mode(sqlx::mysql::MySqlSslMode::Required)
                });
            }
            if backend_name(url) == "MySQL" {
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
                    backend_name(url)
                )))
            })?;
            db.connection.set(connection).map_err(|_| {
                Error::Config(ConfigError::Invalid(
                    "database connection pool was initialized more than once".into(),
                ))
            })?;
            Ok(())
        })
    }
}

fn validate_config(config: &DatabaseConfig) -> Result<()> {
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
        let backend = backend_name(url);
        let enabled = (backend == "PostgreSQL" && cfg!(feature = "db-postgres"))
            || (backend == "MySQL" && cfg!(feature = "db-mysql"));
        if !enabled {
            return Err(Error::Config(ConfigError::Invalid(format!(
                "{backend} database URL requires the matching openrust-db feature"
            ))));
        }
    }

    Ok(())
}

fn backend_name(url: &str) -> &'static str {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        "PostgreSQL"
    } else if url.starts_with("mysql://") {
        "MySQL"
    } else {
        "unsupported"
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
}
