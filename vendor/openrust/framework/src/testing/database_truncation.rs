use std::{collections::BTreeMap, io};

use tokio::task::JoinHandle;

use crate::database::{
    Connection, DatabaseManager, migrations::Migrator, schema::TableInfo, test_events,
    test_transaction,
};

use super::{
    TestApp,
    app::run_artisan,
    database::select_connections,
    database_migrations::{
        TestResult, fresh_migration_arguments, migration_arguments, migration_command_result,
    },
};

/// Options for migrating, truncating, and reseeding a managed MySQL test schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseTruncationOptions {
    /// Include views in the first rebuild.
    pub drop_views: bool,
    /// Run the default seeder after each reset.
    pub seed: bool,
    /// Run this named seeder in place of the default.
    pub seeder: Option<String>,
    /// Select the connections to truncate. An empty list uses the default.
    pub connections_to_truncate: Vec<String>,
    /// Apply these table filters unless the connection has its own filters.
    pub tables: DatabaseTruncationTables,
    /// Select table filters by connection name.
    pub tables_by_connection: BTreeMap<String, DatabaseTruncationTables>,
}

/// Table filters for a database truncation. Match catalog names with or without their schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseTruncationTables {
    /// With a nonempty list, truncate only these tables.
    pub only: Vec<String>,
    /// With no inclusion list, retain these tables and the migration ledger.
    pub except: Vec<String>,
}

pub(super) struct TruncationState {
    connection: Connection,
    migrated: bool,
    pending: Option<JoinHandle<TestResult<()>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's DatabaseTruncation API")]
impl TestApp {
    /// Rebuilds the default schema on the first call, then clears and reseeds the
    /// selected schemas before subsequent tests on this fixture.
    pub async fn truncateDatabaseTables(
        &mut self,
        options: DatabaseTruncationOptions,
    ) -> TestResult<()> {
        self.boot().await?;
        let manager = self.app().make::<DatabaseManager>().await?;
        let default = manager.connection(None)?;
        let selected = select_connections(
            &manager,
            options.connections_to_truncate.iter().map(String::as_str),
        )?;
        test_transaction::validate(&selected)?;
        if !self.owns_mysql_schema(&default)? || test_transaction::active(&default) {
            return Err(io::Error::other("database truncation requires an owned default MySQL schema outside a test transaction").into());
        }
        for connection in &selected {
            if !self.owns_mysql_schema(connection)? {
                return Err(
                    io::Error::other("database truncation requires owned MySQL schemas").into(),
                );
            }
        }
        self.settle_database_migrations().await?;
        self.wait_for_refresh().await;
        self.refreshed = None;
        self.finish_truncation().await?;
        let migrated = self
            .truncation
            .as_ref()
            .is_some_and(|state| state.migrated && state.connection.same_query_source(&default));
        let ledger = self
            .app()
            .make::<Migrator>()
            .await?
            .migration_table()
            .to_owned();
        let app = self.app().share();
        let connection = default.clone();
        let pending = tokio::spawn(async move {
            if !migrated {
                connection.select("SELECT 1", &[]).await?;
                let args = fresh_migration_arguments(
                    &connection,
                    options.drop_views,
                    options.seed,
                    options.seeder.as_deref(),
                );
                return migration_command_result("migrate:fresh", run_artisan(app, args).await);
            }
            for target in selected {
                let filters = options
                    .tables_by_connection
                    .get(target.getName())
                    .unwrap_or(&options.tables);
                truncate_connection(&target, filters, &ledger).await?;
            }
            let seeder = options
                .seeder
                .as_deref()
                .or_else(|| options.seed.then_some(crate::database::seeder::DEFAULT));
            if let Some(seeder) = seeder {
                let mut args = migration_arguments("db:seed", &connection);
                args.push(format!("--class={seeder}").into());
                migration_command_result("db:seed", run_artisan(app, args).await)?;
            }
            Ok(())
        });
        self.truncation = Some(TruncationState {
            connection: default,
            migrated,
            pending: Some(pending),
        });
        self.finish_truncation().await
    }

    pub(super) async fn finish_truncation(&mut self) -> TestResult<()> {
        let Some(state) = &mut self.truncation else {
            return Ok(());
        };
        let Some(pending) = &mut state.pending else {
            return Ok(());
        };
        let result = pending.await;
        state.pending = None;
        result??;
        state.migrated = true;
        Ok(())
    }

    pub(super) async fn invalidate_truncation(&mut self) -> TestResult<()> {
        self.finish_truncation().await?;
        self.truncation = None;
        Ok(())
    }
}

async fn truncate_connection(
    connection: &Connection,
    filters: &DatabaseTruncationTables,
    ledger: &str,
) -> TestResult<()> {
    let (schema, table) = ledger
        .rsplit_once('.')
        .map_or((connection.getDatabaseName(), ledger), |(schema, name)| {
            (schema, name)
        });
    let ledger = format!("{schema}.{}{table}", connection.getTablePrefix());
    let database = connection.getDatabaseName().to_owned();
    let filters = filters.clone();
    let builder = connection.getSchemaBuilder();
    test_events::without_connections(vec![connection.clone()], async move {
        builder
            .withoutForeignKeyConstraints(move |session| {
                Box::pin(async move {
                    let tables = session
                        .getSchemaBuilder()
                        .getTables(Some(&database))
                        .await?;
                    for table in tables
                        .into_iter()
                        .filter(|table| filters.matches(table, &ledger))
                    {
                        session.truncate_nonempty_catalog_table(&table).await?;
                    }
                    Ok(())
                })
            })
            .await
    })
    .await?;
    Ok(())
}

impl DatabaseTruncationTables {
    fn matches(&self, table: &TableInfo, ledger: &str) -> bool {
        let matches = |names: &[String]| {
            names
                .iter()
                .any(|name| name == &table.name || name == &table.schema_qualified_name)
        };
        if self.only.is_empty() {
            table.schema_qualified_name != ledger && !matches(&self.except)
        } else {
            matches(&self.only)
        }
    }
}
