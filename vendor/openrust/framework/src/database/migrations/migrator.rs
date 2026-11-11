use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::database::{DatabaseError, DatabaseManager, DatabaseResult};

use super::{
    DatabaseMigrationRepository, MigrationContext, MigrationEntry, MigrationRegistry,
    MigrationRepositoryInterface, MigrationResult,
};

/// Runs and rolls back compiled migrations against a configured ledger.
pub struct Migrator {
    manager: Arc<DatabaseManager>,
    registry: Arc<MigrationRegistry>,
    table: String,
    root: PathBuf,
    execution: tokio::sync::Mutex<()>,
}

#[derive(Default)]
pub struct RunOptions {
    pub pretend: bool,
    pub step: bool,
}

#[derive(Default)]
pub struct RollbackOptions {
    pub pretend: bool,
    pub step: Option<u64>,
    pub batch: Option<u64>,
}

pub struct MigrationOutcome {
    pub name: String,
    pub result: MigrationResult,
    pub statements: Vec<String>,
}

pub struct MigrationStatus {
    pub name: String,
    pub batch: Option<u64>,
}

impl Migrator {
    pub fn new(
        manager: Arc<DatabaseManager>,
        registry: Arc<MigrationRegistry>,
        table: String,
        root: PathBuf,
    ) -> Self {
        Self {
            manager,
            registry,
            table,
            root,
            execution: tokio::sync::Mutex::new(()),
        }
    }

    fn repository(&self, database: Option<&str>) -> DatabaseMigrationRepository {
        DatabaseMigrationRepository::new(self.manager.clone(), database, &self.table)
    }

    fn files(&self, paths: &[PathBuf]) -> DatabaseResult<BTreeMap<String, &MigrationEntry>> {
        let mut files = BTreeMap::new();
        for entry in self.registry.0 {
            let path = self.root.join(entry.path);
            if !paths.is_empty() && !paths.iter().any(|wanted| path.starts_with(wanted)) {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .filter(|name| !name.is_empty())
                .ok_or(DatabaseError::InvalidMigration("file name"))?;
            if files.insert(name.to_owned(), entry).is_some() {
                return Err(DatabaseError::InvalidMigration("duplicate file name"));
            }
        }
        Ok(files)
    }

    pub async fn install(&self, database: Option<&str>) -> DatabaseResult<bool> {
        let _guard = self.execution.lock().await;
        let mut repository = self.repository(database);
        if repository.repository_exists().await? {
            return Ok(false);
        }
        repository.create_repository().await?;
        Ok(true)
    }

    pub async fn status(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
    ) -> DatabaseResult<Option<Vec<MigrationStatus>>> {
        let mut repository = self.repository(database);
        if !repository.repository_exists().await? {
            return Ok(None);
        }
        let batches: BTreeMap<_, _> = repository
            .get_migration_batches()
            .await?
            .into_iter()
            .collect();
        Ok(Some(
            self.files(paths)?
                .into_keys()
                .map(|name| {
                    let batch = batches.get(&name).copied();
                    MigrationStatus { name, batch }
                })
                .collect(),
        ))
    }

    pub async fn run(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        options: RunOptions,
    ) -> DatabaseResult<Vec<MigrationOutcome>> {
        let _guard = self.execution.lock().await;
        self.run_pending(database, paths, options).await
    }

    async fn run_pending(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        options: RunOptions,
    ) -> DatabaseResult<Vec<MigrationOutcome>> {
        let files = self.files(paths)?;
        let mut repository = self.repository(database);
        if !repository.repository_exists().await? {
            repository.create_repository().await?;
        }
        let ran: BTreeSet<_> = repository.get_ran().await?.into_iter().collect();
        let mut batch = repository.get_next_batch_number().await?;
        let mut outcomes = Vec::new();
        for (name, entry) in files {
            if ran.contains(&name) {
                continue;
            }
            let mut context = MigrationContext::new(
                self.manager
                    .connection(entry.migration.getConnection().or(database))?,
                options.pretend,
            );
            if !options.pretend
                && !entry
                    .migration
                    .shouldRun(&mut context)
                    .await
                    .map_err(|error| migration_error(&name, error))?
            {
                outcomes.push(MigrationOutcome {
                    name,
                    result: MigrationResult::Skipped,
                    statements: Vec::new(),
                });
                continue;
            }
            entry
                .migration
                .up(&mut context)
                .await
                .map_err(|error| migration_error(&name, error))?;
            if !options.pretend {
                repository
                    .log(&name, batch)
                    .await
                    .map_err(|error| migration_error(&name, error))?;
                if options.step {
                    batch = batch
                        .checked_add(1)
                        .ok_or(DatabaseError::InvalidMigration("batch overflow"))?;
                }
            }
            outcomes.push(MigrationOutcome {
                name,
                result: MigrationResult::Success,
                statements: context.take_statements(),
            });
        }
        Ok(outcomes)
    }

    pub async fn rollback(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        options: RollbackOptions,
    ) -> DatabaseResult<Vec<MigrationOutcome>> {
        let _guard = self.execution.lock().await;
        self.rollback_records(database, paths, options, false).await
    }

    pub async fn reset(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        pretend: bool,
    ) -> DatabaseResult<Vec<MigrationOutcome>> {
        let _guard = self.execution.lock().await;
        self.rollback_records(
            database,
            paths,
            RollbackOptions {
                pretend,
                ..Default::default()
            },
            true,
        )
        .await
    }

    pub async fn refresh(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        step: Option<u64>,
    ) -> DatabaseResult<(Vec<MigrationOutcome>, Vec<MigrationOutcome>)> {
        let _guard = self.execution.lock().await;
        let down = self
            .rollback_records(
                database,
                paths,
                RollbackOptions {
                    step,
                    ..Default::default()
                },
                step.is_none(),
            )
            .await?;
        let up = self
            .run_pending(database, paths, RunOptions::default())
            .await?;
        Ok((down, up))
    }

    async fn rollback_records(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        options: RollbackOptions,
        all: bool,
    ) -> DatabaseResult<Vec<MigrationOutcome>> {
        let mut repository = self.repository(database);
        if !repository.repository_exists().await? {
            return Ok(Vec::new());
        }
        let records = if all {
            repository.records().await?
        } else if let Some(step) = options.step.filter(|step| *step > 0) {
            repository.get_migrations(step).await?
        } else if let Some(batch) = options.batch.filter(|batch| *batch > 0) {
            repository.get_migrations_by_batch(batch).await?
        } else {
            repository.get_last().await?
        };
        let files = self.files(paths)?;
        let mut outcomes = Vec::new();
        for record in records {
            let name = &record.migration;
            let Some(entry) = files.get(name) else {
                outcomes.push(MigrationOutcome {
                    name: name.clone(),
                    result: MigrationResult::Skipped,
                    statements: Vec::new(),
                });
                continue;
            };
            let mut context = MigrationContext::new(
                self.manager
                    .connection(entry.migration.getConnection().or(database))?,
                options.pretend,
            );
            entry
                .migration
                .down(&mut context)
                .await
                .map_err(|error| migration_error(name, error))?;
            if !options.pretend {
                repository
                    .delete(&record)
                    .await
                    .map_err(|error| migration_error(name, error))?;
            }
            outcomes.push(MigrationOutcome {
                name: name.clone(),
                result: MigrationResult::Success,
                statements: context.take_statements(),
            });
        }
        Ok(outcomes)
    }

    pub async fn wipe(&self, database: Option<&str>, drop_views: bool) -> DatabaseResult<()> {
        let _guard = self.execution.lock().await;
        self.wipe_schema(database, drop_views).await
    }

    pub async fn fresh(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        step: bool,
        drop_views: bool,
    ) -> DatabaseResult<Vec<MigrationOutcome>> {
        let _guard = self.execution.lock().await;
        self.files(paths)?;
        if self.repository(database).repository_exists().await? {
            self.wipe_schema(database, drop_views).await?;
        }
        self.run_pending(
            database,
            paths,
            RunOptions {
                pretend: false,
                step,
            },
        )
        .await
    }

    async fn wipe_schema(&self, database: Option<&str>, drop_views: bool) -> DatabaseResult<()> {
        let connection = self.manager.connection(database)?;
        if connection.grammar.is_mysql() {
            return connection.wipe_mysql_schema(drop_views).await;
        }
        let grammar = connection.grammar.clone();
        connection.sqlite_schema_transaction(move |transaction, _| Box::pin(async move {
            let rows = transaction.select(
                "select name, type from pragma_table_list where schema = 'main' and type in ('table','virtual','view')                  and substr(name,1,7) <> 'sqlite_' order by type = 'view' desc, name",
                &[],
            ).await?;
            for row in &rows {
                let Some(crate::database::Value::Text(name)) = row.get("name") else {
                    return Err(DatabaseError::InvalidSchema("catalog table name"));
                };
                let kind = match row.get("type") {
                    Some(crate::database::Value::Text(kind)) if kind == "view" && drop_views => "view",
                    Some(crate::database::Value::Text(kind)) if matches!(kind.as_str(), "table" | "virtual") => "table",
                    _ => continue,
                };
                transaction.statement(&format!("drop {kind} main.{}", grammar.quote(name)?), &[]).await?;
            }
            Ok(())
        })).await
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn migration_error(migration: &str, source: DatabaseError) -> DatabaseError {
    DatabaseError::MigrationFailed {
        migration: migration.to_owned(),
        source: Box::new(source),
    }
}
