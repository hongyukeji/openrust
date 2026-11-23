use std::{
    error::Error,
    ffi::OsString,
    io,
    panic::{AssertUnwindSafe, resume_unwind},
    sync::{Arc, Weak},
};

use futures_util::FutureExt as _;
use tokio::task::JoinHandle;

use crate::database::{Connection, DatabaseFuture, DatabaseResult, test_transaction};

use super::{TestApp, TestCommand};

pub(super) type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
type MigrationTask = JoinHandle<DatabaseResult<TestCommand>>;

/// Options for rebuilding the managed test schema before a migration test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseMigrationsOptions {
    /// Include views when rebuilding the schema.
    pub drop_views: bool,
    /// Run the default seeder after migration.
    pub seed: bool,
    /// Run this named seeder in place of the default.
    pub seeder: Option<String>,
}

pub(super) struct MigrationRun {
    connection: Connection,
    active: Weak<()>,
    preparation: Option<MigrationTask>,
    rollback: Option<MigrationTask>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl TestApp {
    /// Rebuilds the managed default MySQL schema, runs one test, and rolls back
    /// the migration batch. The callback can commit its own transactions.
    ///
    /// If the future is cancelled, close the fixture or run another database
    /// reset before reusing the schema.
    pub async fn runDatabaseMigrations<T>(
        &mut self,
        options: DatabaseMigrationsOptions,
        test: impl for<'a> FnOnce(&'a mut TestApp) -> DatabaseFuture<'a, TestResult<T>>,
    ) -> TestResult<T> {
        let connection = self.database().await?;
        if !self.owns_mysql_schema(&connection)? || test_transaction::active(&connection) {
            return Err(io::Error::other(
                "runDatabaseMigrations requires the owned default connection outside a test transaction",
            )
            .into());
        }
        self.settle_database_migrations().await?;
        self.invalidate_truncation().await?;
        self.wait_for_refresh().await;
        self.refreshed = None;

        let active = Arc::new(());
        let arguments = fresh_migration_arguments(
            &connection,
            options.drop_views,
            options.seed,
            options.seeder.as_deref(),
        );
        let preparation = self.spawn_migration_command(connection.clone(), Vec::new(), arguments);
        self.migration_run = Some(MigrationRun {
            connection,
            active: Arc::downgrade(&active),
            preparation: Some(preparation),
            rollback: None,
        });
        let outcome = AssertUnwindSafe(async {
            self.finish_database_migration_preparation().await?;
            test(self).await
        })
        .catch_unwind()
        .await;
        let rollback = self.rollback_database_migrations().await;
        match outcome {
            Ok(result) => finish_database_work(result, rollback),
            Err(panic) => resume_unwind(panic),
        }
    }

    pub(super) async fn settle_database_migrations(&mut self) -> TestResult<()> {
        let Some(run) = &self.migration_run else {
            return Ok(());
        };
        if run.active.upgrade().is_some() || test_transaction::active(&run.connection) {
            return Err(io::Error::other("the database migration test is still active").into());
        }
        let preparation = self.finish_database_migration_preparation().await;
        let rollback = self.rollback_database_migrations().await;
        finish_database_work(preparation, rollback)
    }

    async fn finish_database_migration_preparation(&mut self) -> TestResult<()> {
        let Some(run) = &mut self.migration_run else {
            return Ok(());
        };
        let Some(task) = &mut run.preparation else {
            return Ok(());
        };
        let result = task.await;
        run.preparation = None;
        migration_command_result("migrate:fresh", result??)
    }

    async fn rollback_database_migrations(&mut self) -> TestResult<()> {
        let Some(run) = &self.migration_run else {
            return Ok(());
        };
        if run.rollback.is_none() {
            let connection = run.connection.clone();
            let arguments = migration_arguments("migrate:rollback", &connection);
            let task = self.spawn_migration_command(connection, Vec::new(), arguments);
            self.migration_run.as_mut().unwrap().rollback = Some(task);
        }
        let run = self.migration_run.as_mut().unwrap();
        let result = run.rollback.as_mut().unwrap().await;
        run.rollback = None;
        migration_command_result("migrate:rollback", result??)?;
        self.migration_run = None;
        Ok(())
    }
}

pub(super) fn fresh_migration_arguments(
    connection: &Connection,
    drop_views: bool,
    seed: bool,
    seeder: Option<&str>,
) -> Vec<OsString> {
    let mut arguments = migration_arguments("migrate:fresh", connection);
    if drop_views {
        arguments.push("--drop-views".into());
    }
    if let Some(seeder) = seeder {
        arguments.push(format!("--seeder={seeder}").into());
    } else if seed {
        arguments.push("--seed".into());
    }
    arguments
}

pub(super) fn migration_arguments(command: &str, connection: &Connection) -> Vec<OsString> {
    vec![
        command.into(),
        format!("--database={}", connection.getName()).into(),
        "--no-interaction".into(),
    ]
}

pub(super) fn migration_command_result(command: &str, result: TestCommand) -> TestResult<()> {
    if result.status == 0 {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{command} exited with {}: {}",
        result.status,
        result.stderr.trim()
    ))
    .into())
}

pub(super) fn finish_database_work<T>(
    result: TestResult<T>,
    cleanup: TestResult<()>,
) -> TestResult<T> {
    if let (Err(error), Err(cleanup)) = (&result, &cleanup) {
        return Err(io::Error::other(format!("{error}\n{cleanup}")).into());
    }
    cleanup?;
    result
}
