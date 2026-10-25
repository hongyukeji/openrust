//! Per-test database creation for PostgreSQL and MySQL.

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use openrust_core::{Error, Result};
use openrust_db::{DatabaseBackend, sea_orm_migration::MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use url::Url;

static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A database isolated to one integration test.
///
/// Create one with [`TestDatabase::create`] and use [`url`](Self::url) in the application's
/// `[database].url`. Call [`close`](Self::close) to wait for cleanup; dropping the guard schedules
/// best-effort cleanup on the active Tokio runtime.
pub struct TestDatabase {
    admin_url: String,
    database_name: String,
    database_url: String,
    connection: Option<DatabaseConnection>,
    cleaned: bool,
}

impl TestDatabase {
    /// Creates a uniquely named database and applies the application's migrations.
    ///
    /// The URL must connect to an existing database and the user must be allowed to create and
    /// drop databases. PostgreSQL uses a temporary migrated template; MySQL replays migrations
    /// into the isolated database.
    pub async fn create<M>(admin_url: &str) -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        let parsed = Url::parse(admin_url).map_err(Error::internal)?;
        let backend = DatabaseBackend::from_url(admin_url)?;
        if !backend.is_enabled() {
            return Err(Error::custom(
                "database_feature_disabled",
                format!("enable the openrust-test {} feature", backend.feature()),
            ));
        }
        let database_name = unique_database_name();
        let database_url = url_for_database(&parsed, &database_name);
        let admin = connect(admin_url).await?;

        let setup = match backend {
            DatabaseBackend::PostgreSql => {
                #[cfg(feature = "db-postgres")]
                {
                    create_postgres_database::<M>(&admin, &parsed, &database_name).await
                }
                #[cfg(not(feature = "db-postgres"))]
                {
                    Err(Error::custom(
                        "database_feature_disabled",
                        "enable the openrust-test db-postgres feature",
                    ))
                }
            }
            DatabaseBackend::MySql => {
                #[cfg(feature = "db-mysql")]
                {
                    create_mysql_database::<M>(&admin, &database_url, &database_name).await
                }
                #[cfg(not(feature = "db-mysql"))]
                {
                    Err(Error::custom(
                        "database_feature_disabled",
                        "enable the openrust-test db-mysql feature",
                    ))
                }
            }
        };

        if let Err(error) = setup {
            let _ = drop_database(&admin, &database_name).await;
            let _ = admin.close().await;
            return Err(error);
        }
        if let Err(error) = admin.close().await {
            if let Ok(cleanup) = connect(admin_url).await {
                let _ = drop_database(&cleanup, &database_name).await;
                let _ = cleanup.close().await;
            }
            return Err(Error::internal(error));
        }

        let connection = match connect(&database_url).await {
            Ok(connection) => connection,
            Err(error) => {
                let cleanup = connect(admin_url).await;
                if let Ok(cleanup) = cleanup {
                    let _ = drop_database(&cleanup, &database_name).await;
                    let _ = cleanup.close().await;
                }
                return Err(error);
            }
        };

        Ok(Self {
            admin_url: admin_url.to_owned(),
            database_name,
            database_url,
            connection: Some(connection),
            cleaned: false,
        })
    }

    /// Returns the connection URL for configuring the application under test.
    pub fn url(&self) -> &str {
        &self.database_url
    }

    /// Returns a clone of the isolated SeaORM connection pool.
    pub fn connection(&self) -> Result<DatabaseConnection> {
        self.connection
            .clone()
            .ok_or_else(|| Error::custom("test_database_closed", "test database is already closed"))
    }

    /// Closes the connection pool and removes the isolated database.
    pub async fn close(mut self) -> Result<()> {
        let connection = self.connection.take().ok_or_else(|| {
            Error::custom("test_database_closed", "test database is already closed")
        })?;
        let close_result = connection.close().await.map_err(Error::internal);
        let cleanup_result = async {
            let admin = connect(&self.admin_url).await?;
            let drop_result = drop_database(&admin, &self.database_name).await;
            let admin_close_result = admin.close().await.map_err(Error::internal);
            drop_result?;
            admin_close_result
        }
        .await;
        self.cleaned = close_result.is_ok() && cleanup_result.is_ok();
        close_result?;
        cleanup_result
    }
}

impl fmt::Debug for TestDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestDatabase")
            .field("database_url", &"[REDACTED]")
            .field("database_name", &self.database_name)
            .field("connected", &self.connection.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let connection = self.connection.take();
        let admin_url = self.admin_url.clone();
        let database_name = self.database_name.clone();
        runtime.spawn(async move {
            if let Some(connection) = connection {
                let _ = connection.close().await;
            }
            if let Ok(admin) = connect(&admin_url).await {
                let _ = drop_database(&admin, &database_name).await;
                let _ = admin.close().await;
            }
        });
    }
}

#[cfg(feature = "db-postgres")]
async fn create_postgres_database<M>(
    admin: &DatabaseConnection,
    base_url: &Url,
    database_name: &str,
) -> Result<()>
where
    M: MigratorTrait + Send + Sync + 'static,
{
    let template_name = format!("{database_name}_template");
    execute(admin, &format!("CREATE DATABASE {template_name}")).await?;
    let template_url = url_for_database(base_url, &template_name);
    let migration = async {
        let template = connect(&template_url).await?;
        let result = M::up(&template, None).await.map_err(Error::internal);
        let close_result = template.close().await.map_err(Error::internal);
        result?;
        close_result
    }
    .await;
    if let Err(error) = migration {
        let _ = execute(admin, &format!("DROP DATABASE IF EXISTS {template_name}")).await;
        return Err(error);
    }
    if let Err(error) = execute(
        admin,
        &format!("CREATE DATABASE {database_name} TEMPLATE {template_name}"),
    )
    .await
    {
        let _ = execute(admin, &format!("DROP DATABASE IF EXISTS {template_name}")).await;
        return Err(error);
    }
    execute(admin, &format!("DROP DATABASE {template_name}")).await
}

#[cfg(feature = "db-mysql")]
async fn create_mysql_database<M>(
    admin: &DatabaseConnection,
    database_url: &str,
    database_name: &str,
) -> Result<()>
where
    M: MigratorTrait + Send + Sync + 'static,
{
    execute(admin, &format!("CREATE DATABASE {database_name}")).await?;
    let database = connect(database_url).await?;
    let result = M::up(&database, None).await.map_err(Error::internal);
    let close_result = database.close().await.map_err(Error::internal);
    result?;
    close_result
}

async fn connect(url: &str) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_owned());
    options.max_connections(2).min_connections(0);
    #[cfg(feature = "db-mysql")]
    if DatabaseBackend::from_url(url)? == DatabaseBackend::MySql {
        options.map_sqlx_mysql_opts(|mysql| mysql.ssl_mode(sqlx::mysql::MySqlSslMode::Required));
        options.after_connect(|connection| {
            Box::pin(async move {
                connection
                    .execute_unprepared("SET time_zone = '+00:00'")
                    .await?;
                Ok(())
            })
        });
    }
    Database::connect(options).await.map_err(Error::internal)
}

async fn execute(connection: &DatabaseConnection, sql: &str) -> Result<()> {
    connection
        .execute_unprepared(sql)
        .await
        .map(|_| ())
        .map_err(Error::internal)
}

async fn drop_database(connection: &DatabaseConnection, name: &str) -> Result<()> {
    execute(connection, &format!("DROP DATABASE IF EXISTS {name}")).await
}

fn url_for_database(base: &Url, name: &str) -> String {
    let mut url = base.clone();
    url.set_path(name);
    url.to_string()
}

fn unique_database_name() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_micros());
    let sequence = DATABASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("openrust_{}_{}_{}", std::process::id(), timestamp, sequence)
}

#[cfg(test)]
mod tests {
    use super::{unique_database_name, url_for_database};
    use url::Url;

    #[test]
    fn database_url_replacement_preserves_auth_and_query_options() {
        let base = Url::parse("postgres://tester:secret@localhost/app_db?sslmode=require");
        assert!(base.is_ok());
        let Ok(base) = base else {
            return;
        };
        let isolated = Url::parse(&url_for_database(&base, "openrust_test_1"));
        assert!(isolated.is_ok());
        let Ok(isolated) = isolated else {
            return;
        };
        assert_eq!(isolated.username(), "tester");
        assert_eq!(isolated.password(), Some("secret"));
        assert_eq!(isolated.path(), "/openrust_test_1");
        assert_eq!(isolated.query(), Some("sslmode=require"));
    }

    #[test]
    fn generated_database_names_fit_postgresql_identifier_limits() {
        let name = unique_database_name();
        assert!(name.len() <= 54);
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        );
    }
}
