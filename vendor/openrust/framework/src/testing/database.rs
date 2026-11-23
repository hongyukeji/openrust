use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::File,
    io,
    path::Path,
    sync::Arc,
};

use serde_json::json;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseFuture, DatabaseManager, DatabaseResult, seeder,
        test_mysql::{MySqlTestDatabase, MySqlTestOwners},
        test_transaction,
    },
    foundation::{Application, configuration::ApplicationBuilder},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{
    TestApp, TestCommand,
    database_migrations::{finish_database_work, fresh_migration_arguments},
};

const FILE: &str = "testing.sqlite";

/// MySQL migration and seeding options for `TestApp::refresh_database`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshDatabaseOptions {
    /// Include views when rebuilding the schema.
    pub drop_views: bool,
    /// Run the default seeder after migration.
    pub seed: bool,
    /// Run this named seeder instead of the default.
    pub seeder: Option<String>,
    /// Named MySQL connections to roll back. An empty list selects the default.
    pub connections_to_transact: Vec<String>,
    /// Mute the selected connections' events during migration preparation.
    pub without_migration_events: bool,
}

pub(super) enum RefreshState {
    Preparing(
        RefreshDatabaseOptions,
        tokio::task::JoinHandle<DatabaseResult<TestCommand>>,
    ),
    Ready(RefreshDatabaseOptions),
}

struct Sqlite;

struct Mysql {
    connections: BTreeMap<String, serde_json::Value>,
    default: String,
}

impl ServiceProvider for Mysql {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            let config = app.config()?;
            config.set("database.connections", json!(self.connections))?;
            config.set("database.default", json!(self.default))?;
            Ok(())
        })
    }
}

impl ServiceProvider for Sqlite {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            let config = app.config()?;
            config.set(
                "database.connections",
                json!({
                    "sqlite": {"driver": "sqlite", "database": FILE}
                }),
            )?;
            config.set("database.default", json!("sqlite"))?;
            Ok(())
        })
    }
}

impl TestApp {
    /// Creates a test application with a private SQLite file.
    ///
    /// The fixture replaces the configured connections during provider registration.
    /// Migrations and seeders can be registered through the application builder.
    pub fn sqlite(
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let fixture =
            Self::create(|builder| configure(builder.with_providers(vec![Box::new(Sqlite)])))?;
        File::options()
            .write(true)
            .create_new(true)
            .open(fixture.root().join(FILE))?;
        Ok(fixture)
    }

    /// Creates an application owning a new schema on a dedicated MySQL test server.
    pub async fn mysql(
        admin_url: &str,
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::mysql_connections(&[("mysql", admin_url)], configure).await
    }

    /// Creates an owned MySQL test schema for each named connection.
    /// The first connection is the application's default.
    pub async fn mysql_connections(
        connections: &[(&str, &str)],
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::create_mysql(connections, None, configure).await
    }

    /// Creates an owned test schema and records it in an existing private directory.
    pub async fn mysql_with_recovery(
        admin_url: &str,
        directory: &Path,
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::mysql_connections_with_recovery(&[("mysql", admin_url)], directory, configure).await
    }

    /// Creates owned schemas with independent recovery receipts for each named connection.
    pub async fn mysql_connections_with_recovery(
        connections: &[(&str, &str)],
        directory: &Path,
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::create_mysql(connections, Some(directory), configure).await
    }

    /// Cleans up one abandoned schema on the original dedicated MySQL test server.
    /// A missing receipt is a no-op. Ownership, busy-schema, or lease failures
    /// retain the receipt so cleanup can be retried after inspection.
    pub async fn recover_mysql(admin_url: &str, receipt: &Path) -> io::Result<()> {
        MySqlTestDatabase::recover(admin_url, receipt).await
    }

    async fn create_mysql(
        connections: &[(&str, &str)],
        journal: Option<&Path>,
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut names = BTreeSet::new();
        let Some(&(default, _)) = connections.first() else {
            return Err(io::Error::other("at least one MySQL test connection is required").into());
        };
        if connections
            .iter()
            .any(|(name, _)| name.is_empty() || name.contains("::") || !names.insert(*name))
        {
            return Err(io::Error::other(
                "MySQL test connection names must be unique and unqualified",
            )
            .into());
        }
        let mut databases = Vec::with_capacity(connections.len());
        let mut configs = BTreeMap::new();
        let mut owners = BTreeMap::new();
        for &(name, url) in connections {
            let database = match MySqlTestDatabase::create(url, journal).await {
                Ok(database) => database,
                Err(error) => {
                    close_mysql(databases).await?;
                    return Err(error.into());
                }
            };
            configs.insert(
                name.to_owned(),
                json!({"url": database.url, "acquire_timeout": 2}),
            );
            owners.insert(name.to_owned(), database.owner.clone());
            databases.push(database);
        }
        let provider = Box::new(Mysql {
            connections: configs,
            default: default.into(),
        });
        let mut fixture =
            match Self::create(|builder| configure(builder.with_providers(vec![provider]))) {
                Ok(fixture) => fixture,
                Err(error) => {
                    close_mysql(databases).await?;
                    return Err(error);
                }
            };
        fixture
            .app()
            .container()
            .instance(Arc::new(MySqlTestOwners(owners)))?;
        fixture.mysql = databases;
        Ok(fixture)
    }

    /// Releases the test application and waits for owned schema cleanup.
    pub async fn close(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let Some(RefreshState::Preparing(_, task)) = self.refreshed.take() {
            // Keep the schema lease until any in-flight migration unwinds.
            let _ = task.await;
        }
        let migration = self.settle_database_migrations().await;
        let truncation = self.finish_truncation().await;
        let queries = self.query_counts.finish().map_err(Into::into);
        let mysql = std::mem::take(&mut self.mysql);
        drop(self);
        let cleanup = close_mysql(mysql).await.map_err(Into::into);
        let resets = finish_database_work(migration, truncation);
        let checks = finish_database_work(resets, queries);
        finish_database_work(checks, cleanup)
    }

    /// Boots the test application and resolves its default database connection.
    pub async fn database(&mut self) -> Result<Connection, Box<dyn Error + Send + Sync>> {
        self.database_on(None).await
    }

    pub(super) async fn database_on(
        &mut self,
        connection: Option<&str>,
    ) -> Result<Connection, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(self
            .app()
            .make::<DatabaseManager>()
            .await?
            .connection(connection)?)
    }

    /// Runs the registered default `DatabaseSeeder` through Artisan.
    pub async fn seed(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.seed_with(&[seeder::DEFAULT]).await
    }

    /// Runs the named seeders in the supplied order on the default connection.
    pub async fn seed_with(
        &mut self,
        seeders: &[&str],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.run_seeders(seeders, None).await
    }

    /// Runs seeders on the selected connection. Pass `None` for the default seeder.
    pub async fn seed_on(
        &mut self,
        connection: &str,
        seeders: Option<&[&str]>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.run_seeders(seeders.unwrap_or(&[seeder::DEFAULT]), Some(connection))
            .await
    }

    async fn run_seeders(
        &mut self,
        seeders: &[&str],
        connection: Option<&str>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        for name in seeders {
            let args = [
                "db:seed".into(),
                "--no-interaction".into(),
                format!("--class={name}"),
            ];
            let command = self
                .artisan(
                    args.into_iter()
                        .chain(connection.map(|name| format!("--database={name}"))),
                )
                .await;
            if command.status != 0 {
                return Err(io::Error::other(format!(
                    "db:seed {name} exited with {}: {}",
                    command.status,
                    command.stderr.trim()
                ))
                .into());
            }
        }
        Ok(())
    }

    /// Runs a test against the default MySQL connection and rolls back its writes.
    ///
    /// Prepare the schema and any shared fixtures before entering the callback.
    /// Requests dispatched by this test application and its Artisan commands
    /// participate when they use its default connection on the current task.
    pub async fn database_transaction<T>(
        &mut self,
        test: impl for<'a> FnOnce(
            &'a mut TestApp,
        )
            -> DatabaseFuture<'a, Result<T, Box<dyn Error + Send + Sync>>>,
    ) -> Result<T, Box<dyn Error + Send + Sync>> {
        self.database_transaction_on(&[], test).await
    }

    /// Runs one test in a rollback transaction on each selected MySQL connection.
    /// An empty list selects the default.
    pub async fn database_transaction_on<T>(
        &mut self,
        names: &[&str],
        test: impl for<'a> FnOnce(
            &'a mut TestApp,
        )
            -> DatabaseFuture<'a, Result<T, Box<dyn Error + Send + Sync>>>,
    ) -> Result<T, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        let manager = self.app().make::<DatabaseManager>().await?;
        let connections = select_connections(&manager, names.iter().copied())?;
        test_transaction::run(connections, async { test(self).await }).await?
    }

    /// Prepares a managed MySQL schema, then runs one rollback-isolated test.
    ///
    /// The first use, a change of options, or an interrupted transaction
    /// triggers a fresh migration. A test error with a successful rollback
    /// preserves the prepared baseline for the next call.
    pub async fn refresh_database<T>(
        &mut self,
        options: RefreshDatabaseOptions,
        test: impl for<'a> FnOnce(
            &'a mut TestApp,
        )
            -> DatabaseFuture<'a, Result<T, Box<dyn Error + Send + Sync>>>,
    ) -> Result<T, Box<dyn Error + Send + Sync>> {
        if self.mysql.is_empty() {
            return Err(
                io::Error::other("refresh_database requires a managed MySQL test schema").into(),
            );
        }
        self.boot().await?;
        let manager = self.app().make::<DatabaseManager>().await?;
        let connection = manager.connection(None)?;
        let connections = select_connections(
            &manager,
            options.connections_to_transact.iter().map(String::as_str),
        )?;
        test_transaction::validate(&connections)?;
        if !self.owns_mysql_schema(&connection)?
            || test_transaction::active(&connection)
            || connections.iter().any(test_transaction::active)
        {
            return Err(io::Error::other(
                "refresh_database requires the owned default connection outside a test transaction",
            )
            .into());
        }
        self.settle_database_migrations().await?;
        self.invalidate_truncation().await?;
        self.wait_for_refresh().await;
        let prepared = self.refreshed.take();
        if !matches!(prepared, Some(RefreshState::Ready(ready)) if ready == options) {
            let arguments = fresh_migration_arguments(
                &connection,
                options.drop_views,
                options.seed,
                options.seeder.as_deref(),
            );
            let muted = if options.without_migration_events {
                connections.clone()
            } else {
                Vec::new()
            };
            let task = self.spawn_migration_command(connection, muted, arguments);
            self.refreshed = Some(RefreshState::Preparing(options.clone(), task));
            self.finish_refresh().await?;
            self.refreshed = None;
        }
        let result = test_transaction::run(connections, async { test(self).await }).await?;
        self.refreshed = Some(RefreshState::Ready(options));
        result
    }

    pub(super) fn owns_mysql_schema(&self, connection: &Connection) -> DatabaseResult<bool> {
        if self.mysql.is_empty() {
            return Ok(false);
        }
        let owners = self
            .app()
            .container()
            .make::<MySqlTestOwners>()
            .map_err(crate::database::DatabaseError::Unavailable)?;
        Ok(owners
            .0
            .get(connection.get_name())
            .is_some_and(|owner| owner.is_schema(connection.get_database_name())))
    }

    pub(super) async fn wait_for_refresh(&mut self) {
        if matches!(self.refreshed, Some(RefreshState::Preparing(..))) {
            // A cancelled caller can leave its migration running.
            let _ = self.finish_refresh().await;
        }
    }

    async fn finish_refresh(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let Some(RefreshState::Preparing(options, task)) = &mut self.refreshed else {
            return Ok(());
        };
        let result = task.await;
        let options = options.clone();
        self.refreshed = None;
        let command = result??;
        if command.status != 0 {
            return Err(io::Error::other(format!(
                "refresh_database migration exited with {}: {}",
                command.status,
                command.stderr.trim()
            ))
            .into());
        }
        self.refreshed = Some(RefreshState::Ready(options));
        Ok(())
    }
}

pub(super) fn select_connections<'a>(
    manager: &DatabaseManager,
    mut names: impl Iterator<Item = &'a str>,
) -> crate::database::DatabaseResult<Vec<Connection>> {
    let Some(first) = names.next() else {
        return Ok(vec![manager.connection(None)?]);
    };
    std::iter::once(first)
        .chain(names)
        .map(|name| manager.connection(Some(name)))
        .collect()
}

async fn close_mysql(databases: Vec<MySqlTestDatabase>) -> io::Result<()> {
    let mut result = Ok(());
    for database in databases {
        let closed = database.close().await;
        result = result.and(closed);
    }
    result
}
