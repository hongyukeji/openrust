use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::File,
    io,
    sync::Arc,
};

use serde_json::json;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseFuture, DatabaseManager, Value,
        test_mysql::{MySqlTestDatabase, MySqlTestOwners},
        test_transaction,
    },
    foundation::{Application, configuration::ApplicationBuilder},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{TestApp, TestCommand};

const FILE: &str = "testing.sqlite";

/// MySQL migration and seeding options for `TestApp::refreshDatabase`.
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
}

pub(super) enum RefreshState {
    Preparing(RefreshDatabaseOptions, tokio::task::JoinHandle<TestCommand>),
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

#[allow(
    non_snake_case,
    reason = "Matches Laravel's database testing assertions"
)]
impl TestApp {
    /// Creates a test application with a private SQLite file.
    ///
    /// The fixture replaces the configured connections during provider registration.
    /// Migrations and seeders can be registered through the application builder.
    pub fn sqlite(
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let fixture =
            Self::create(|builder| configure(builder.withProviders(vec![Box::new(Sqlite)])))?;
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
        Self::mysqlConnections(&[("mysql", admin_url)], configure).await
    }

    /// Creates an owned MySQL test schema for each named connection.
    /// The first connection is the application's default.
    pub async fn mysqlConnections(
        connections: &[(&str, &str)],
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
            let database = match MySqlTestDatabase::create(url).await {
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
            match Self::create(|builder| configure(builder.withProviders(vec![provider]))) {
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
        let mysql = std::mem::take(&mut self.mysql);
        drop(self);
        close_mysql(mysql).await?;
        Ok(())
    }

    /// Boots the test application and resolves its default database connection.
    pub async fn database(&mut self) -> Result<Connection, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(self
            .app()
            .make::<DatabaseManager>()
            .await?
            .connection(None)?)
    }

    /// Runs a test against the default MySQL connection and rolls back its writes.
    ///
    /// Prepare the schema and any shared fixtures before entering the callback.
    /// Requests dispatched by this test application and its Artisan commands
    /// participate when they use its default connection on the current task.
    pub async fn databaseTransaction<T>(
        &mut self,
        test: impl for<'a> FnOnce(
            &'a mut TestApp,
        )
            -> DatabaseFuture<'a, Result<T, Box<dyn Error + Send + Sync>>>,
    ) -> Result<T, Box<dyn Error + Send + Sync>> {
        self.databaseTransactionOn(&[], test).await
    }

    /// Runs one test in a rollback transaction on each selected MySQL connection.
    /// An empty list selects the default.
    pub async fn databaseTransactionOn<T>(
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
    pub async fn refreshDatabase<T>(
        &mut self,
        options: RefreshDatabaseOptions,
        test: impl for<'a> FnOnce(
            &'a mut TestApp,
        )
            -> DatabaseFuture<'a, Result<T, Box<dyn Error + Send + Sync>>>,
    ) -> Result<T, Box<dyn Error + Send + Sync>> {
        if self.mysql.is_empty() {
            return Err(
                io::Error::other("refreshDatabase requires a managed MySQL test schema").into(),
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
        let owners = self.app().container().make::<MySqlTestOwners>()?;
        if !owners
            .0
            .get(connection.getName())
            .is_some_and(|owner| owner.is_schema(connection.getDatabaseName()))
            || test_transaction::active(&connection)
            || connections.iter().any(test_transaction::active)
        {
            return Err(io::Error::other(
                "refreshDatabase requires the owned default connection outside a test transaction",
            )
            .into());
        }
        if matches!(self.refreshed, Some(RefreshState::Preparing(..))) {
            // A cancelled caller can leave its migration running. Settle it
            // before retrying or starting a differently configured refresh.
            let _ = self.finish_refresh().await;
        }
        let prepared = self.refreshed.take();
        if !matches!(prepared, Some(RefreshState::Ready(ready)) if ready == options) {
            // Open the owned pool before Artisan can consider missing-database creation.
            connection.select("SELECT 1", &[]).await?;
            let mut arguments = vec![
                "migrate:fresh".to_owned(),
                format!("--database={}", connection.getName()),
                "--no-interaction".to_owned(),
            ];
            if options.drop_views {
                arguments.push("--drop-views".into());
            }
            if let Some(seeder) = &options.seeder {
                arguments.push(format!("--seeder={seeder}"));
            } else if options.seed {
                arguments.push("--seed".into());
            }
            let task = self.spawn_artisan(arguments.into_iter().map(Into::into).collect());
            self.refreshed = Some(RefreshState::Preparing(options.clone(), task));
            self.finish_refresh().await?;
            self.refreshed = None;
        }
        let result = test_transaction::run(connections, async { test(self).await }).await?;
        self.refreshed = Some(RefreshState::Ready(options));
        result
    }

    async fn finish_refresh(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let Some(RefreshState::Preparing(options, task)) = &mut self.refreshed else {
            return Ok(());
        };
        let result = task.await;
        let options = options.clone();
        self.refreshed = None;
        let command = result?;
        if command.status != 0 {
            return Err(io::Error::other(format!(
                "refreshDatabase migration exited with {}: {}",
                command.status,
                command.stderr.trim()
            ))
            .into());
        }
        self.refreshed = Some(RefreshState::Ready(options));
        Ok(())
    }

    /// Asserts that a table contains a row matching the supplied values.
    pub async fn assertDatabaseHas(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        assert!(
            self.database_contains(table, columns).await?,
            "no matching row in {table}"
        );
        Ok(())
    }

    /// Asserts that a table has no row matching the supplied values.
    pub async fn assertDatabaseMissing(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        assert!(
            !self.database_contains(table, columns).await?,
            "matching row found in {table}"
        );
        Ok(())
    }

    /// Asserts that a table contains exactly the specified number of rows.
    pub async fn assertDatabaseCount(
        &mut self,
        table: &str,
        expected: u64,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        assert_eq!(
            self.database().await?.table(table).count("*").await?,
            expected,
            "row count for {table}"
        );
        Ok(())
    }

    async fn database_contains(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let connection = self.database().await?;
        let query = columns
            .iter()
            .fold(connection.table(table), |query, (column, value)| {
                query.whereNullSafeEquals(column, value.clone())
            });
        Ok(query.exists().await?)
    }
}

fn select_connections<'a>(
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
