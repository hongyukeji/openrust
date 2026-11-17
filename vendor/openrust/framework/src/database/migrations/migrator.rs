use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use crate::database::{
    Connection, DatabaseError, DatabaseManager, DatabaseResult,
    events::{
        MigrationEnded, MigrationMethod, MigrationOptions, MigrationSkipped, MigrationStarted,
        MigrationsEnded, MigrationsPruned, MigrationsStarted, NoPendingMigrations, SchemaDumped,
        SchemaLoaded,
    },
};

use super::{
    DatabaseMigrationRepository, MigrationContext, MigrationEntry, MigrationRegistry,
    MigrationRepositoryInterface, MigrationResult, database_creation::MissingDatabase,
    schema_state,
};

type ConfirmDatabaseCreation<'a> = &'a (dyn Fn(&Connection) -> DatabaseResult<bool> + Send + Sync);

/// Runs and rolls back compiled migrations against a configured ledger.
pub struct Migrator {
    manager: Arc<DatabaseManager>,
    registry: Arc<MigrationRegistry>,
    packages: RwLock<Vec<&'static MigrationEntry>>,
    migrations_to_skip: RwLock<BTreeSet<String>>,
    table: String,
    root: PathBuf,
    execution: tokio::sync::Mutex<()>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RunOptions {
    pub pretend: bool,
    pub step: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct WipeOptions {
    pub drop_views: bool,
    pub drop_types: bool,
}

#[derive(Clone, Copy, Debug, Default)]
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

pub(crate) struct RefreshOutcome {
    pub down: Vec<MigrationOutcome>,
    pub up: Vec<MigrationOutcome>,
    pub loaded: bool,
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
            packages: RwLock::default(),
            migrations_to_skip: RwLock::default(),
            table,
            root,
            execution: tokio::sync::Mutex::new(()),
        }
    }

    /// Replaces the pending migrations excluded by this application.
    #[allow(non_snake_case, reason = "Matches Laravel's Migrator API")]
    pub fn withoutMigrations(&self, migrations: &[&str]) {
        let names = migrations
            .iter()
            .filter_map(|migration| Path::new(migration).file_name()?.to_str())
            .map(|name| name.strip_suffix(".rs").unwrap_or(name).to_owned())
            .collect();
        *self
            .migrations_to_skip
            .write()
            .expect("migration exclusions lock poisoned") = names;
    }

    pub(crate) fn add_package_migrations(&self, migrations: &'static [MigrationEntry]) {
        self.packages
            .write()
            .expect("package migrations lock poisoned")
            .extend(migrations);
    }

    pub(crate) fn migration_table(&self) -> &str {
        &self.table
    }

    /// Resolves a migration connection, preferring a configured PostgreSQL direct endpoint.
    #[allow(non_snake_case, reason = "Matches Laravel's Migrator API")]
    pub fn resolveConnection(&self, name: Option<&str>) -> DatabaseResult<Connection> {
        self.manager.migration_connection(name)
    }

    fn repository(&self, database: Option<&str>) -> DatabaseMigrationRepository {
        DatabaseMigrationRepository::new(self.manager.clone(), database, &self.table)
    }

    fn files(&self, paths: &[PathBuf]) -> DatabaseResult<BTreeMap<String, &MigrationEntry>> {
        let mut files = BTreeMap::new();
        let packages = self
            .packages
            .read()
            .expect("package migrations lock poisoned");
        for entry in self.registry.0.iter().chain(packages.iter().copied()) {
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
        Ok(self
            .status_entries(database, paths)
            .await?
            .map(|entries| entries.into_iter().map(|(_, status)| status).collect()))
    }

    pub(crate) async fn status_with_skips(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
    ) -> DatabaseResult<Option<Vec<(MigrationStatus, bool)>>> {
        let Some(entries) = self.status_entries(database, paths).await? else {
            return Ok(None);
        };
        let mut has_pending = false;
        let mut statuses = Vec::with_capacity(entries.len());
        for (entry, status) in entries {
            let mut skipped = false;
            if status.batch.is_none() && !has_pending {
                let connection =
                    self.resolveConnection(entry.migration.getConnection().or(database))?;
                let mut context = MigrationContext::new(connection, false);
                skipped = !entry
                    .migration
                    .shouldRun(&mut context)
                    .await
                    .map_err(|error| migration_error(&status.name, error))?;
                has_pending = !skipped;
            }
            statuses.push((status, skipped));
        }
        Ok(Some(statuses))
    }

    async fn status_entries(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
    ) -> DatabaseResult<Option<Vec<(&MigrationEntry, MigrationStatus)>>> {
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
                .into_iter()
                .map(|(name, entry)| {
                    let batch = batches.get(&name).copied();
                    (entry, MigrationStatus { name, batch })
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
        self.run_with_schema(database, paths, options, None, None)
            .await
            .map(|(rows, _)| rows)
    }

    pub(crate) async fn run_with_schema(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        options: RunOptions,
        schema_path: Option<&Path>,
        confirm_create: Option<ConfirmDatabaseCreation<'_>>,
    ) -> DatabaseResult<(Vec<MigrationOutcome>, bool)> {
        let _guard = self.execution.lock().await;
        self.run_pending(database, paths, options, schema_path, confirm_create)
            .await
    }

    async fn run_pending(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        options: RunOptions,
        schema_path: Option<&Path>,
        confirm_create: Option<ConfirmDatabaseCreation<'_>>,
    ) -> DatabaseResult<(Vec<MigrationOutcome>, bool)> {
        let files = self.files(paths)?;
        let mut repository = self.repository(database);
        let mut installed = match repository.repository_exists().await {
            Ok(installed) => installed,
            Err(error) => {
                let (Some(confirm), Some(missing)) =
                    (confirm_create, MissingDatabase::from_error(&error))
                else {
                    return Err(error);
                };
                let connection = self.resolveConnection(database)?;
                let Some(creation) = missing.prepare(&connection)? else {
                    return Err(error);
                };
                if !confirm(&connection)? {
                    return Err(DatabaseError::DatabaseCreationDeclined);
                }
                creation.create().await?;
                repository.repository_exists().await?
            }
        };
        let mut ran = if installed {
            repository.get_ran().await?
        } else {
            Vec::new()
        };
        let mut loaded = false;
        if ran.is_empty() && !options.pretend {
            let connection = self.resolveConnection(database)?;
            if let Some(path) =
                self.schema_to_load(schema_connection_name(&connection), schema_path)?
            {
                if connection.pools.generation()?.schema_credentials.is_none() {
                    return Err(schema_state::UNSUPPORTED);
                }
                if installed {
                    repository.delete_repository().await?;
                }
                schema_state::load(&connection, &path).await?;
                installed = repository.repository_exists().await?;
                if installed {
                    ran = repository.get_ran().await?;
                }
                self.manager
                    .dispatch_migration_event(SchemaLoaded {
                        connectionName: connection.getName().to_owned(),
                        connection,
                        path,
                    })
                    .await?;
                loaded = true;
            }
        }
        if !installed {
            repository.create_repository().await?;
        }
        let ran: BTreeSet<_> = ran.into_iter().collect();
        let pending: Vec<_> = {
            let excluded = self
                .migrations_to_skip
                .read()
                .expect("migration exclusions lock poisoned");
            files
                .into_iter()
                .filter(|(name, _)| !ran.contains(name) && !excluded.contains(name))
                .collect()
        };
        let method = MigrationMethod::Up;
        if pending.is_empty() {
            self.manager
                .dispatch_migration_event(NoPendingMigrations { method })
                .await?;
            return Ok((Vec::new(), loaded));
        }
        let mut batch = repository.get_next_batch_number().await?;
        let mut outcomes = Vec::with_capacity(pending.len());
        let event_options = MigrationOptions::Up(options);
        self.manager
            .dispatch_migration_event(MigrationsStarted {
                method,
                options: event_options,
            })
            .await?;
        for (name, entry) in pending {
            let mut context = MigrationContext::new(
                self.resolveConnection(entry.migration.getConnection().or(database))?,
                options.pretend,
            );
            if !options.pretend
                && !entry
                    .migration
                    .shouldRun(&mut context)
                    .await
                    .map_err(|error| migration_error(&name, error))?
            {
                self.manager
                    .dispatch_migration_event(MigrationSkipped {
                        migrationName: name.clone(),
                    })
                    .await?;
                outcomes.push(MigrationOutcome {
                    name,
                    result: MigrationResult::Skipped,
                    statements: Vec::new(),
                });
                continue;
            }
            self.run_migration(entry, &name, method, &mut context, options.pretend)
                .await?;
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
        self.manager
            .dispatch_migration_event(MigrationsEnded {
                method,
                options: event_options,
            })
            .await?;
        Ok((outcomes, loaded))
    }

    async fn run_migration(
        &self,
        entry: &MigrationEntry,
        name: &str,
        method: MigrationMethod,
        context: &mut MigrationContext,
        pretend: bool,
    ) -> DatabaseResult<()> {
        let migration = entry.migration;
        context
            .begin_transaction(!pretend && migration.withinTransaction())
            .await
            .map_err(|error| migration_error(name, error))?;
        let result = self
            .run_migration_body(entry, name, method, context, pretend)
            .await;
        let completed = result.is_ok();
        let result = context.finish(result).await;
        if completed {
            result.map_err(|error| migration_error(name, error))
        } else {
            result
        }
    }

    async fn run_migration_body(
        &self,
        entry: &MigrationEntry,
        name: &str,
        method: MigrationMethod,
        context: &mut MigrationContext,
        pretend: bool,
    ) -> DatabaseResult<()> {
        let migration = entry.migration;
        if !pretend {
            self.manager
                .dispatch_migration_event_in(
                    MigrationStarted {
                        migration,
                        method,
                        name: name.to_owned(),
                    },
                    context.transaction(),
                )
                .await?;
        }
        match method {
            MigrationMethod::Up => migration.up(context).await,
            MigrationMethod::Down => migration.down(context).await,
        }
        .map_err(|error| migration_error(name, error))?;
        if !pretend {
            self.manager
                .dispatch_migration_event_in(
                    MigrationEnded {
                        migration,
                        method,
                        name: name.to_owned(),
                    },
                    context.transaction(),
                )
                .await?;
        }
        Ok(())
    }

    #[allow(non_snake_case, reason = "Matches Laravel schema event payloads")]
    pub(crate) async fn dump_schema(
        &self,
        database: Option<&str>,
        path: Option<&Path>,
        include_ledger: bool,
        prune: bool,
    ) -> DatabaseResult<()> {
        let _guard = self.execution.lock().await;
        let connection = self.resolveConnection(database)?;
        let path = match path {
            Some(path) => self.root.join(path),
            None => self.default_schema_path(schema_connection_name(&connection), "sql")?,
        };
        if prune
            && self
                .registry
                .0
                .iter()
                .any(|entry| self.root.join(entry.path) == path)
        {
            return Err(DatabaseError::InvalidSchema(
                "snapshot path is a migration source",
            ));
        }
        let ledger = if include_ledger && self.repository(database).repository_exists().await? {
            Some(self.table.as_str())
        } else {
            None
        };
        schema_state::dump(&connection, &path, ledger).await?;
        let connectionName = connection.getName().to_owned();
        self.manager
            .dispatch_migration_event(SchemaDumped {
                connection: connection.clone(),
                connectionName: connectionName.clone(),
                path,
            })
            .await?;
        if prune {
            self.prune_migration_sources()?;
            self.manager
                .dispatch_migration_event(MigrationsPruned {
                    connection,
                    connectionName,
                    path: self.root.join("database/migrations"),
                })
                .await?;
        }
        Ok(())
    }

    fn default_schema_path(&self, connection: &str, extension: &str) -> DatabaseResult<PathBuf> {
        if connection.is_empty() || connection.contains(['/', '\\']) {
            return Err(DatabaseError::InvalidSchema("snapshot connection name"));
        }
        Ok(self
            .root
            .join("database/schema")
            .join(format!("{connection}-schema.{extension}")))
    }

    fn schema_to_load(
        &self,
        connection: &str,
        override_path: Option<&Path>,
    ) -> DatabaseResult<Option<PathBuf>> {
        let paths = match override_path {
            Some(path) => vec![self.root.join(path)],
            None => vec![
                self.default_schema_path(connection, "dump")?,
                self.default_schema_path(connection, "sql")?,
            ],
        };
        for path in paths {
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => return Ok(Some(path)),
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(DatabaseError::SchemaIo(error)),
            }
        }
        Ok(None)
    }

    fn prune_migration_sources(&self) -> DatabaseResult<()> {
        let directory = self.root.join("database/migrations");
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(DatabaseError::InvalidMigration("source directory")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(DatabaseError::SchemaIo(error)),
        }
        for entry in self.registry.0 {
            let path = self.root.join(entry.path);
            if path.parent() == Some(directory.as_path())
                && path.extension().is_some_and(|ext| ext == "rs")
            {
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(DatabaseError::SchemaIo(error)),
                }
            }
        }
        Ok(())
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
        self.refresh_with_schema(database, paths, step)
            .await
            .map(|result| (result.down, result.up))
    }

    pub(crate) async fn refresh_with_schema(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        step: Option<u64>,
    ) -> DatabaseResult<RefreshOutcome> {
        let _guard = self.execution.lock().await;
        let step = step.filter(|step| *step > 0);
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
        let (up, loaded) = self
            .run_pending(database, paths, RunOptions::default(), None, None)
            .await?;
        Ok(RefreshOutcome { down, up, loaded })
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
        let method = MigrationMethod::Down;
        if records.is_empty() {
            if !all {
                self.manager
                    .dispatch_migration_event(NoPendingMigrations { method })
                    .await?;
            }
            return Ok(Vec::new());
        }
        let files = self.files(paths)?;
        let event_options = MigrationOptions::Down(options);
        self.manager
            .dispatch_migration_event(MigrationsStarted {
                method,
                options: event_options,
            })
            .await?;
        let mut outcomes = Vec::with_capacity(records.len());
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
                self.resolveConnection(entry.migration.getConnection().or(database))?,
                options.pretend,
            );
            self.run_migration(entry, name, method, &mut context, options.pretend)
                .await?;
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
        self.manager
            .dispatch_migration_event(MigrationsEnded {
                method,
                options: event_options,
            })
            .await?;
        Ok(outcomes)
    }

    pub async fn wipe(&self, database: Option<&str>, drop_views: bool) -> DatabaseResult<()> {
        self.wipe_with_options(
            database,
            WipeOptions {
                drop_views,
                ..WipeOptions::default()
            },
        )
        .await
    }

    pub(crate) async fn wipe_with_options(
        &self,
        database: Option<&str>,
        options: WipeOptions,
    ) -> DatabaseResult<()> {
        let _guard = self.execution.lock().await;
        Self::wipe_schema(&self.resolveConnection(database)?, options).await
    }

    pub async fn fresh(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        step: bool,
        drop_views: bool,
    ) -> DatabaseResult<Vec<MigrationOutcome>> {
        self.fresh_with_schema(
            database,
            paths,
            step,
            WipeOptions {
                drop_views,
                ..WipeOptions::default()
            },
            None,
            None,
        )
        .await
        .map(|(rows, _)| rows)
    }

    pub(crate) async fn fresh_with_schema(
        &self,
        database: Option<&str>,
        paths: &[PathBuf],
        step: bool,
        wipe: WipeOptions,
        schema_path: Option<&Path>,
        confirm_create: Option<ConfirmDatabaseCreation<'_>>,
    ) -> DatabaseResult<(Vec<MigrationOutcome>, bool)> {
        let _guard = self.execution.lock().await;
        self.files(paths)?;
        let connection = self.resolveConnection(database)?;
        if wipe.drop_types && !connection.is_postgres() {
            return Err(DatabaseError::UnsupportedSchema(
                "dropping types requires PostgreSQL",
            ));
        }
        match self.repository(database).repository_exists().await {
            Ok(true) => Self::wipe_schema(&connection, wipe).await?,
            Ok(false) => {}
            Err(error)
                if confirm_create.is_some() && MissingDatabase::from_error(&error).is_some() => {}
            Err(error) => return Err(error),
        }
        self.run_pending(
            database,
            paths,
            RunOptions {
                pretend: false,
                step,
            },
            schema_path,
            confirm_create,
        )
        .await
    }

    async fn wipe_schema(connection: &Connection, options: WipeOptions) -> DatabaseResult<()> {
        connection
            .getSchemaBuilder()
            .drop_schema_objects(true, options.drop_views, options.drop_types)
            .await?;
        if connection.is_postgres() {
            connection.disconnect();
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn schema_connection_name(connection: &Connection) -> &str {
    let name = connection.getName();
    if connection.is_postgres() {
        // The direct transport is an endpoint alias for the same snapshot.
        return name.strip_suffix("::direct").unwrap_or(name);
    }
    name
}

fn migration_error(migration: &str, source: DatabaseError) -> DatabaseError {
    DatabaseError::MigrationFailed {
        migration: migration.to_owned(),
        source: Box::new(source),
    }
}
