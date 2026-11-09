//! 基于 SeaORM migration 定义的运行时迁移命令。

mod execution;

#[cfg(all(feature = "db-sqlite", feature = "session"))]
pub(crate) use execution::residency::return_connection as return_sqlite_connection;

#[cfg(feature = "db-sqlite")]
pub(super) fn sqlite_execution_owner() -> bool {
    execution::is_sqlite_owner()
}

use std::cell::Cell;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::console::Command;
use crate::core::Error;
use crate::core::Result;
use crate::foundation::Application;
use crate::foundation::BootContext;
use crate::support::{BoxFuture, ServiceProvider};
use sea_orm_migration::sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use sea_orm_migration::{MigrationTrait, MigratorTrait, SchemaManager};

use crate::database::{Databases, Db};

/// 框架数据库基础设施迁移；业务模块迁移由各模块独立贡献。
pub struct FrameworkMigrator;

#[sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for FrameworkMigrator {
    fn migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
        let migrations: Vec<Box<dyn sea_orm_migration::MigrationTrait>> = vec![
            Box::new(crate::database::DatabaseRateLimitMigration),
            Box::new(crate::audit::DatabaseAuditMigration),
            Box::new(crate::audit::DatabaseAuditTenantMigration),
        ];
        #[cfg(all(feature = "session", not(feature = "auth")))]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(crate::session::SessionMigration));
            migrations
        };
        #[cfg(feature = "auth")]
        let migrations = {
            let mut migrations = migrations;
            migrations.extend([
                Box::new(crate::auth::AuthMigration) as Box<dyn sea_orm_migration::MigrationTrait>,
                Box::new(crate::auth::AuthRefreshTokenMigration),
                Box::new(crate::auth::AuthVerificationCodeMigration),
                Box::new(crate::auth::AuthPolicyMigration),
                Box::new(crate::auth::AuthRememberTokenMigration),
            ]);
            migrations
        };
        #[cfg(feature = "session")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(crate::session::SessionGcMigration));
            migrations
        };
        #[cfg(feature = "signing")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(crate::signing::SignedLinkRevocationMigration));
            migrations
        };
        #[cfg(feature = "auth-fortify")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(
                crate::auth::fortify::FortifyTwoFactorMigration::new(),
            ));
            migrations
        };
        #[cfg(feature = "auth-oidc")]
        let migrations = {
            let mut migrations = migrations;
            migrations.extend([
                Box::new(crate::auth::AuthExternalIdentityMigration)
                    as Box<dyn sea_orm_migration::MigrationTrait>,
                Box::new(crate::auth::AuthExternalSessionMigration),
            ]);
            migrations
        };
        #[cfg(feature = "queue")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(crate::queue::QueueMigration));
            migrations
        };
        #[cfg(feature = "data-exchange-tasks")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(crate::database::CoordinationMigration));
            migrations
        };
        migrations
    }
}

/// 在 Provider boot 阶段贡献 `migrate` 命令。
pub struct MigrationServiceProvider<M> {
    lock_wait_timeout: Duration,
    module: &'static str,
    marker: PhantomData<fn() -> M>,
}

impl<M> MigrationServiceProvider<M> {
    /// 创建使用默认锁等待期限的迁移命令 Provider。
    pub fn new() -> Self {
        Self {
            lock_wait_timeout: super::migration_lock::default_wait_timeout(),
            module: "application",
            marker: PhantomData,
        }
    }

    /// 设置等待其他迁移完成的最长时间。
    #[must_use]
    pub fn with_lock_wait_timeout(mut self, timeout: Duration) -> Self {
        self.lock_wait_timeout = timeout;
        self
    }

    /// 标识所属模块；多模块迁移共享同一 `migrate` 命令与锁。
    #[must_use]
    pub fn for_module(mut self, name: &'static str) -> Self {
        self.module = name;
        self
    }
}

type MigrationFactory = fn() -> Vec<Box<dyn MigrationTrait>>;

#[derive(Clone)]
struct MigrationEntry {
    module: &'static str,
    names: Vec<String>,
    migrations: MigrationFactory,
}

#[derive(Default)]
struct MigrationCatalog(Mutex<Vec<MigrationEntry>>);

impl MigrationCatalog {
    fn add(&self, entry: MigrationEntry) -> Result<()> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| Error::custom("migration_catalog_unavailable", "迁移目录不可用"))?;
        if entries
            .iter()
            .any(|existing| existing.module == entry.module)
        {
            return Err(Error::custom(
                "module_migration_conflict",
                "模块迁移来源重复",
            ));
        }
        let unique_names = entry.names.iter().collect::<HashSet<_>>();
        if unique_names.len() != entry.names.len() {
            return Err(Error::custom(
                "module_migration_conflict",
                "模块内部迁移名称重复",
            ));
        }
        let prior = entries
            .iter()
            .flat_map(|existing| existing.names.iter())
            .collect::<HashSet<_>>();
        if entry.names.iter().any(|name| prior.contains(name)) {
            return Err(Error::custom(
                "module_migration_conflict",
                "模块迁移名称重复",
            ));
        }
        entries.push(entry);
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<MigrationEntry>> {
        self.0
            .lock()
            .map(|entries| entries.clone())
            .map_err(|_| Error::custom("migration_catalog_unavailable", "迁移目录不可用"))
    }
}

impl<M> Default for MigrationServiceProvider<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> ServiceProvider for MigrationServiceProvider<M>
where
    M: MigratorTrait + Send + Sync + 'static,
{
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<MigrationCatalog>() {
                app.instance(Arc::new(MigrationCatalog::default())).await?;
            }
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let names = M::migrations()
                .into_iter()
                .map(|migration| migration.name().to_owned())
                .collect::<Vec<_>>();
            let catalog = app.resolve::<MigrationCatalog>().await?;
            catalog.add(MigrationEntry {
                module: self.module,
                names,
                migrations: M::migrations,
            })?;
            app.command(MigrationCommand {
                lock_wait_timeout: self.lock_wait_timeout,
                catalog,
                resources: app.resources().clone(),
            });
            Ok(())
        })
    }
}

struct MigrationCommand {
    lock_wait_timeout: Duration,
    catalog: Arc<MigrationCatalog>,
    resources: crate::foundation::resources::ResourceRegistrar,
}

const MIGRATE_USAGE: &str = "[--database <name>] [--force] [--no-interaction|-n] [up [steps | --step] | --step | down [steps] | status]";

impl Command for MigrationCommand {
    fn name(&self) -> &'static str {
        "migrate"
    }

    fn about(&self) -> &'static str {
        "执行、回滚或查看数据库迁移"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(MIGRATE_USAGE)
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if args
                .iter()
                .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
            {
                crate::console::command::print_help(self);
                return Ok(());
            }
            let options = MigrationOptions::parse(&args)?;
            options.authorize(app.is_production(), options.consume_confirmation())?;
            let db = if let Some(name) = options.database.as_deref() {
                app.state().resolve::<Databases>().await?.get(name)?
            } else {
                app.state().resolve::<Db>().await?.as_ref().clone()
            };
            #[cfg(not(feature = "db-sqlite"))]
            if matches!(db.backend(), Ok(DbBackend::Sqlite)) {
                return Err(execution::sqlite_feature_disabled());
            }
            let migrator = CatalogMigrator {
                sources: self.catalog.snapshot()?,
            };
            if options.action == MigrationAction::Status {
                let database = db.primary_connection()?;
                let history = migration_history(&database, false).await?;
                print_status(&migrator, &history);
                return Ok(());
            }
            let lock_url = db.migration_lock_url()?.to_owned();
            execution::execute(
                app,
                &self.resources,
                db,
                lock_url,
                migrator,
                options.action,
                self.lock_wait_timeout,
            )
            .await
        })
    }
}

struct CatalogMigrator {
    sources: Vec<MigrationEntry>,
}

impl CatalogMigrator {
    fn migrations(&self) -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = self
            .sources
            .iter()
            .flat_map(|source| (source.migrations)())
            .collect::<Vec<_>>();
        migrations.sort_by(|left, right| left.name().cmp(right.name()));
        migrations
    }
}

#[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
async fn run_migrator(migrator: &CatalogMigrator, db: &Db, action: MigrationAction) -> Result<()> {
    run_migrator_owned(
        migrator,
        db,
        action,
        &execution::MigrationCancellation::uncancelled(),
    )
    .await
}

async fn run_migrator_owned(
    migrator: &CatalogMigrator,
    db: &Db,
    action: MigrationAction,
    cancellation: &execution::MigrationCancellation,
) -> Result<()> {
    cancellation.check()?;
    let database = db.connection()?;
    let mut history = migration_history(
        &database,
        matches!(action, MigrationAction::Up(_) | MigrationAction::UpStep),
    )
    .await?;
    let migrations = migrator.migrations();
    let applied = history
        .iter()
        .map(|row| row.version.clone())
        .collect::<HashSet<_>>();
    match action {
        MigrationAction::Up(_) | MigrationAction::UpStep => {
            let pending = migrations
                .iter()
                .filter(|migration| !applied.contains(migration.name()));
            let limit = match action {
                MigrationAction::Up(Some(steps)) => usize::try_from(steps).unwrap_or(usize::MAX),
                _ => usize::MAX,
            };
            let first_batch = next_batch(&history)?;
            for (index, migration) in pending.take(limit).enumerate() {
                cancellation.check()?;
                let batch = if matches!(action, MigrationAction::UpStep) {
                    first_batch
                        .checked_add(i64::try_from(index).map_err(|_| {
                            Error::custom("migration_batch_overflow", "迁移批次超出可执行范围")
                        })?)
                        .ok_or_else(|| {
                            Error::custom("migration_batch_overflow", "迁移批次超出可执行范围")
                        })?
                } else {
                    first_batch
                };
                db.table_names()
                    .scope(apply_migration(&database, migration.as_ref(), batch))
                    .await?;
                history.push(BatchRow {
                    version: migration.name().to_owned(),
                    batch,
                });
            }
            print_migrations("Migrations applied.\n");
        }
        MigrationAction::Down(steps) => {
            history.sort_by(|left, right| {
                right
                    .batch
                    .cmp(&left.batch)
                    .then_with(|| right.version.cmp(&left.version))
            });
            let latest = history.first().map(|row| row.batch);
            let selected = history.into_iter().filter(|row| match steps {
                Some(_) => true,
                None => Some(row.batch) == latest,
            });
            let limit = steps
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or(usize::MAX);
            for row in selected.take(limit) {
                cancellation.check()?;
                let Some(migration) = migrations
                    .iter()
                    .find(|migration| migration.name() == row.version)
                else {
                    continue;
                };
                db.table_names()
                    .scope(revert_migration(&database, migration.as_ref()))
                    .await?;
            }
            print_migrations("Migrations rolled back.\n");
        }
        MigrationAction::Status => {
            print_status(migrator, &history);
        }
    }
    Ok(())
}

struct BatchRow {
    version: String,
    batch: i64,
}

fn print_status(migrator: &CatalogMigrator, history: &[BatchRow]) {
    use std::fmt::Write as _;

    let migrations = migrator.migrations();
    if migrations.is_empty() {
        print_migrations("No migrations registered.\n");
        return;
    }
    let applied: HashSet<_> = history.iter().map(|row| row.version.as_str()).collect();
    let mut output = String::new();
    for migration in migrations {
        let status = if applied.contains(migration.name()) {
            "Applied"
        } else {
            "Pending"
        };
        let _ = writeln!(output, "{}: {}", migration.name(), status);
    }
    print_migrations(&output);
}

async fn migration_history(db: &DatabaseConnection, install: bool) -> Result<Vec<BatchRow>> {
    if SchemaManager::new(db)
        .has_table("seaql_migrations")
        .await
        .map_err(Error::internal)?
        && db
            .query_one_raw(Statement::from_string(
                db.get_database_backend(),
                "SELECT version FROM seaql_migrations LIMIT 1".to_owned(),
            ))
            .await
            .map_err(Error::internal)?
            .is_some()
    {
        return Err(Error::custom(
            "migration_history_upgrade_required",
            "旧迁移历史不为空；请先在隔离环境转换历史，不能把已迁移数据库当作空库",
        ));
    }
    if install {
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS openrust_migrations (version VARCHAR(255) PRIMARY KEY, batch BIGINT NOT NULL)",
        )
        .await
        .map_err(Error::internal)?;
    } else if !SchemaManager::new(db)
        .has_table("openrust_migrations")
        .await
        .map_err(Error::internal)?
    {
        return Err(Error::custom(
            "migration_history_missing",
            "找不到迁移历史表",
        ));
    }
    let rows = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT version, batch FROM openrust_migrations".to_owned(),
        ))
        .await
        .map_err(Error::internal)?;
    rows.into_iter()
        .map(|row| {
            Ok(BatchRow {
                version: row.try_get("", "version").map_err(Error::internal)?,
                batch: row.try_get("", "batch").map_err(Error::internal)?,
            })
        })
        .collect()
}

fn next_batch(history: &[BatchRow]) -> Result<i64> {
    history
        .iter()
        .map(|row| row.batch)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::custom("migration_batch_overflow", "迁移批次超出可执行范围"))
}

fn history_statement(
    backend: DbBackend,
    sql: &'static str,
    values: Vec<sea_orm_migration::sea_orm::Value>,
) -> Statement {
    Statement::from_sql_and_values(backend, sql, values)
}

async fn insert_history<C: ConnectionTrait>(db: &C, version: &str, batch: i64) -> Result<()> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => "INSERT INTO openrust_migrations (version, batch) VALUES ($1, $2)",
        _ => "INSERT INTO openrust_migrations (version, batch) VALUES (?, ?)",
    };
    db.execute_raw(history_statement(
        backend,
        sql,
        vec![version.to_owned().into(), batch.into()],
    ))
    .await
    .map_err(Error::internal)?;
    Ok(())
}

async fn delete_history<C: ConnectionTrait>(db: &C, version: &str) -> Result<()> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => "DELETE FROM openrust_migrations WHERE version = $1",
        _ => "DELETE FROM openrust_migrations WHERE version = ?",
    };
    db.execute_raw(history_statement(
        backend,
        sql,
        vec![version.to_owned().into()],
    ))
    .await
    .map_err(Error::internal)?;
    Ok(())
}

fn migration_uses_transaction(migration: &dyn MigrationTrait, backend: DbBackend) -> bool {
    migration
        .use_transaction()
        .unwrap_or(backend == DbBackend::Postgres)
}

async fn apply_migration(
    db: &DatabaseConnection,
    migration: &dyn MigrationTrait,
    batch: i64,
) -> Result<()> {
    if migration_uses_transaction(migration, db.get_database_backend()) {
        let transaction = db.begin().await.map_err(Error::internal)?;
        let outcome = async {
            migration
                .up(&SchemaManager::new(&transaction))
                .await
                .map_err(Error::internal)?;
            insert_history(&transaction, migration.name(), batch).await
        }
        .await;
        if let Err(error) = outcome {
            return rollback_migration(transaction, error).await;
        }
        commit_migration(transaction).await?;
    } else {
        migration
            .up(&SchemaManager::new(db))
            .await
            .map_err(Error::internal)?;
        insert_history(db, migration.name(), batch).await?;
    }
    Ok(())
}

async fn revert_migration(db: &DatabaseConnection, migration: &dyn MigrationTrait) -> Result<()> {
    if migration_uses_transaction(migration, db.get_database_backend()) {
        let transaction = db.begin().await.map_err(Error::internal)?;
        let outcome = async {
            migration
                .down(&SchemaManager::new(&transaction))
                .await
                .map_err(Error::internal)?;
            delete_history(&transaction, migration.name()).await
        }
        .await;
        if let Err(error) = outcome {
            return rollback_migration(transaction, error).await;
        }
        commit_migration(transaction).await?;
    } else {
        migration
            .down(&SchemaManager::new(db))
            .await
            .map_err(Error::internal)?;
        delete_history(db, migration.name()).await?;
    }
    Ok(())
}

async fn rollback_migration(
    transaction: sea_orm::DatabaseTransaction,
    original: Error,
) -> Result<()> {
    if transaction.rollback().await.is_err() {
        tracing::error!("迁移失败后的事务回滚失败；不能确认事务已结束");
        return Err(Error::custom(
            "migration_rollback_failed",
            "迁移失败且无法确认事务回滚完成",
        ));
    }
    Err(original)
}

async fn commit_migration(transaction: sea_orm::DatabaseTransaction) -> Result<()> {
    // SeaORM commit(self) 失败后只能 Drop 排队回滚；执行所有者必须识别此不确定状态。
    transaction.commit().await.map_err(|_| {
        Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "migration_commit_failed",
            "迁移事务提交失败，不能确认事务已结束",
        )
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MigrationAction {
    Up(Option<u32>),
    UpStep,
    Down(Option<u32>),
    Status,
}

#[derive(PartialEq, Eq)]
pub(crate) struct MigrationOptions {
    database: Option<String>,
    action: MigrationAction,
    force: bool,
    no_interaction: bool,
}

tokio::task_local! {
    static CONFIRMED_MIGRATION: Cell<Option<MigrationOptions>>;
}

pub(crate) async fn with_confirmation<T>(
    approval: Option<MigrationOptions>,
    entry: impl std::future::Future<Output = T>,
) -> T {
    CONFIRMED_MIGRATION.scope(Cell::new(approval), entry).await
}

pub(crate) fn confirm_before_boot(
    name: &str,
    args: &[String],
    config: &crate::config::repository::ConfigRepository,
    container: &crate::container::Container,
) -> Result<Option<MigrationOptions>> {
    if name != "migrate"
        || !container.contains::<MigrationCatalog>()
        || args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Ok(None);
    }
    // 其他多词命令保留命令仓储的最长名称分派。
    let Ok(options) = MigrationOptions::parse(args) else {
        return Ok(None);
    };
    let app: crate::config::AppConfig = config.section("app")?;
    if !options.needs_confirmation(app.env.is_production()) {
        return Ok(None);
    }
    options.authorize(true, false)?;
    Ok(Some(options))
}

impl MigrationOptions {
    fn needs_confirmation(&self, production: bool) -> bool {
        production && self.action != MigrationAction::Status && !self.force
    }

    fn authorize(&self, production: bool, confirmed: bool) -> Result<()> {
        if self.needs_confirmation(production)
            && !confirmed
            && !crate::console::command::confirm_in_terminal(
                self.no_interaction,
                "应用处于生产环境。是否继续数据库迁移？[y/N]",
                "migration_confirmation_failed",
            )?
        {
            return Err(Error::custom(
                "migration_confirmation_required",
                "已取消生产迁移。无人值守部署请显式指定 --force。",
            ));
        }
        Ok(())
    }

    fn consume_confirmation(&self) -> bool {
        CONFIRMED_MIGRATION
            .try_with(|approval| {
                let allowed = approval.take();
                if allowed.as_ref() == Some(self) {
                    true
                } else {
                    approval.set(allowed);
                    false
                }
            })
            .unwrap_or(false)
    }

    fn parse(args: &[String]) -> Result<Self> {
        let invalid = || {
            Error::custom(
                "invalid_migration_command",
                format!("用法：migrate {MIGRATE_USAGE}"),
            )
        };
        let mut database = None;
        let mut force = false;
        let mut no_interaction = false;
        let mut action_args = Vec::new();
        let mut args = args.iter().map(String::as_str);
        while let Some(arg) = args.next() {
            match arg {
                "--force" => force = true,
                "--no-interaction" | "-n" => no_interaction = true,
                flag if flag == "--database" || flag.starts_with("--database=") => {
                    let value = flag
                        .strip_prefix("--database=")
                        .or_else(|| args.next())
                        .filter(|value| !value.is_empty() && !value.starts_with('-'))
                        .ok_or_else(invalid)?;
                    if database.replace(value.to_owned()).is_some() {
                        return Err(invalid());
                    }
                }
                _ => action_args.push(arg),
            }
        }
        let action = match action_args.as_slice() {
            [] | ["up"] => MigrationAction::Up(None),
            ["--step"] | ["up", "--step"] => MigrationAction::UpStep,
            ["up", count] => MigrationAction::Up(Some(parse_steps(count).ok_or_else(invalid)?)),
            ["down"] => MigrationAction::Down(None),
            ["down", count] => MigrationAction::Down(Some(parse_steps(count).ok_or_else(invalid)?)),
            ["status"] if !force => MigrationAction::Status,
            _ => return Err(invalid()),
        };
        Ok(Self {
            database,
            action,
            force,
            no_interaction,
        })
    }
}

fn parse_steps(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|steps| *steps > 0)
}

#[allow(clippy::print_stdout)]
fn print_migrations(output: &str) {
    print!("{output}");
}

#[cfg(test)]
mod tests {
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    use super::{CatalogMigrator, MigrationEntry, run_migrator};
    use super::{MigrationAction, MigrationCatalog, MigrationOptions, MigrationServiceProvider};
    use crate::core::Result;
    use crate::foundation::ProviderBuilder;
    use sea_orm_migration::prelude::*;

    struct FirstMigrator;
    struct SecondMigrator;
    struct SharedMigration;
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct OrderedMigration(&'static str);
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct OrderedA;
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct OrderedB;
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct EmptyMigrator;
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct FailingAfterDdl;
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    struct RecoverAfterDdl;

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    impl MigrationName for FailingAfterDdl {
        fn name(&self) -> &'static str {
            "m20261028_000004_failure_probe"
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    impl MigrationName for RecoverAfterDdl {
        fn name(&self) -> &'static str {
            "m20261028_000004_failure_probe"
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    async fn create_probe(manager: &SchemaManager<'_>) -> std::result::Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("r2m_failure_probe"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null())
                    .to_owned(),
            )
            .await
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for FailingAfterDdl {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            create_probe(manager).await?;
            Err(DbErr::Custom("injected migration failure".to_owned()))
        }

        async fn down(&self, _: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for RecoverAfterDdl {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            create_probe(manager).await
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .drop_table(
                    Table::drop()
                        .table(Alias::new("r2m_failure_probe"))
                        .to_owned(),
                )
                .await
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    fn failing_migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(FailingAfterDdl)]
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    fn recovery_migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(RecoverAfterDdl)]
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    impl MigrationName for OrderedMigration {
        fn name(&self) -> &str {
            self.0
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for OrderedMigration {
        async fn up(&self, _: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for OrderedA {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![
                Box::new(OrderedMigration("m20261028_000001_a")),
                Box::new(OrderedMigration("m20261028_000003_a")),
            ]
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for OrderedB {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(OrderedMigration("m20261028_000002_b"))]
        }
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }

    impl MigrationName for SharedMigration {
        fn name(&self) -> &'static str {
            "m20261028_000001_shared"
        }
    }

    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for SharedMigration {
        async fn up(&self, _: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }
    }

    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for FirstMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(SharedMigration)]
        }
    }

    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for SecondMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(SharedMigration)]
        }
    }

    #[tokio::test]
    async fn module_migration_name_conflict_fails_before_ready() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(MigrationServiceProvider::<FirstMigrator>::new().for_module("first"))
            .provider(MigrationServiceProvider::<SecondMigrator>::new().for_module("second"));
        let error = builder
            .start(figment::Figment::new())
            .await
            .err()
            .ok_or_else(|| crate::core::Error::custom("expected_conflict", "迁移冲突未被拒绝"))?;
        assert_eq!(error.code(), "module_migration_conflict");
        Ok(())
    }

    #[tokio::test]
    async fn one_enabled_module_contributes_one_migration_command() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(MigrationServiceProvider::<FirstMigrator>::new().for_module("first"));
        let runtime = builder.start(figment::Figment::new()).await?;
        assert_eq!(
            runtime
                .container()
                .resolve::<MigrationCatalog>()
                .await?
                .snapshot()?
                .len(),
            1
        );
        assert_eq!(
            runtime
                .commands()
                .filter(|command| command.name() == "migrate")
                .count(),
            1
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[tokio::test]
    async fn global_order_batches_and_disabled_latest_batch_are_preserved() -> Result<()> {
        use crate::database::{DatabaseServiceProvider, Db};
        use crate::testing::TestDatabase;
        use figment::providers::Serialized;

        let Ok(admin_url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping migration order test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
        let mut builder = ProviderBuilder::new();
        builder.provider(DatabaseServiceProvider);
        let runtime = builder
            .start(figment::Figment::from(Serialized::defaults(
                serde_json::json!({
                    "database": {"url": database.url()}
                }),
            )))
            .await?;
        let db = runtime.container().resolve::<Db>().await?;
        let a = MigrationEntry {
            module: "a",
            names: OrderedA::migrations()
                .into_iter()
                .map(|item| item.name().to_owned())
                .collect(),
            migrations: OrderedA::migrations,
        };
        let b = MigrationEntry {
            module: "b",
            names: OrderedB::migrations()
                .into_iter()
                .map(|item| item.name().to_owned())
                .collect(),
            migrations: OrderedB::migrations,
        };
        let catalog = CatalogMigrator {
            sources: vec![a.clone(), b],
        };
        run_migrator(&catalog, &db, MigrationAction::Up(Some(1))).await?;
        run_migrator(&catalog, &db, MigrationAction::Up(Some(1))).await?;
        let connection = database.connection()?;
        assert_eq!(
            migration_versions(&connection).await?,
            ["m20261028_000001_a", "m20261028_000002_b"]
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        let mut restarted = ProviderBuilder::new();
        restarted.provider(DatabaseServiceProvider);
        let runtime = restarted
            .start(figment::Figment::from(Serialized::defaults(
                serde_json::json!({
                    "database": {"url": database.url()}
                }),
            )))
            .await?;
        let db = runtime.container().resolve::<Db>().await?;
        let disabled = CatalogMigrator { sources: vec![a] };
        run_migrator(&disabled, &db, MigrationAction::Down(None)).await?;
        assert_eq!(migration_versions(&connection).await?.len(), 2);
        run_migrator(&catalog, &db, MigrationAction::UpStep).await?;
        run_migrator(&catalog, &db, MigrationAction::Down(None)).await?;
        let after_latest = migration_versions(&connection).await?;
        assert_eq!(after_latest.len(), 2);
        assert!(
            after_latest
                .iter()
                .all(|version| version != "m20261028_000003_a")
        );
        run_migrator(&catalog, &db, MigrationAction::Down(Some(1))).await?;
        let after_one = migration_versions(&connection).await?;
        assert_eq!(after_one.len(), 1);
        assert_eq!(after_one[0], "m20261028_000001_a");
        run_migrator(&catalog, &db, MigrationAction::Up(None)).await?;
        run_migrator(&catalog, &db, MigrationAction::Down(None)).await?;
        let after_batch = migration_versions(&connection).await?;
        assert_eq!(after_batch.len(), 1);
        assert_eq!(after_batch[0], "m20261028_000001_a");
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        database.close().await?;
        Ok(())
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[tokio::test]
    async fn failed_migration_has_no_history_row_and_restart_can_recover() -> Result<()> {
        use crate::database::{DatabaseServiceProvider, Db};
        use crate::testing::TestDatabase;
        use figment::providers::Serialized;
        use sea_orm_migration::sea_orm::DbBackend;

        let Ok(admin_url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping migration failure recovery: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
        let source = || {
            figment::Figment::from(Serialized::defaults(serde_json::json!({
                "database": {"url": database.url()}
            })))
        };
        let mut builder = ProviderBuilder::new();
        builder.provider(DatabaseServiceProvider);
        let runtime = builder.start(source()).await?;
        let db = runtime.container().resolve::<Db>().await?;
        let failing = CatalogMigrator {
            sources: vec![MigrationEntry {
                module: "probe",
                names: vec!["m20261028_000004_failure_probe".to_owned()],
                migrations: failing_migrations,
            }],
        };
        assert!(
            run_migrator(&failing, &db, MigrationAction::Up(None))
                .await
                .is_err()
        );
        let connection = database.connection()?;
        assert_eq!(migration_versions(&connection).await?, Vec::<String>::new());
        let table_exists = SchemaManager::new(&connection)
            .has_table("r2m_failure_probe")
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(
            table_exists,
            connection.get_database_backend() == DbBackend::MySql
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;

        let mut restarted = ProviderBuilder::new();
        restarted.provider(DatabaseServiceProvider);
        let runtime = restarted.start(source()).await?;
        let db = runtime.container().resolve::<Db>().await?;
        let recovery = CatalogMigrator {
            sources: vec![MigrationEntry {
                module: "probe",
                names: vec!["m20261028_000004_failure_probe".to_owned()],
                migrations: recovery_migrations,
            }],
        };
        run_migrator(&recovery, &db, MigrationAction::Up(None)).await?;
        assert_eq!(
            migration_versions(&connection).await?,
            ["m20261028_000004_failure_probe"]
        );
        run_migrator(&recovery, &db, MigrationAction::Down(None)).await?;
        assert_eq!(migration_versions(&connection).await?, Vec::<String>::new());
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        database.close().await?;
        Ok(())
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[tokio::test]
    async fn legacy_history_is_rejected_before_mutating_new_history() -> Result<()> {
        use crate::database::DatabaseServiceProvider;
        use crate::testing::TestDatabase;
        use figment::providers::Serialized;
        use sea_orm_migration::sea_orm::ConnectionTrait;

        let Ok(admin_url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping legacy migration protection: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
        let connection = database.connection()?;
        connection
            .execute_unprepared(
                "INSERT INTO seaql_migrations (version, applied_at) VALUES ('legacy_probe', 0)",
            )
            .await
            .map_err(crate::core::Error::internal)?;
        let mut builder = ProviderBuilder::new();
        builder
            .provider(DatabaseServiceProvider)
            .provider(MigrationServiceProvider::<OrderedA>::new());
        let runtime = builder
            .start(figment::Figment::from(Serialized::defaults(
                serde_json::json!({
                    "app": {"env": "testing"},
                    "database": {"url": database.url(), "max_connections": 1, "min_connections": 1}
                }),
            )))
            .await?;
        let app = crate::foundation::Application::from_provider_runtime(runtime)?;
        let error = app
            .run_command("migrate", Vec::new())
            .await
            .err()
            .ok_or_else(|| {
                crate::core::Error::custom("expected_legacy_rejection", "旧历史未被拒绝")
            })?;
        assert_eq!(error.code(), "migration_history_upgrade_required");
        assert!(
            !SchemaManager::new(&connection)
                .has_table("openrust_migrations")
                .await
                .map_err(crate::core::Error::internal)?
        );
        assert!(
            connection
                .query_one_raw(sea_orm_migration::sea_orm::Statement::from_string(
                    connection.get_database_backend(),
                    "SELECT version FROM seaql_migrations WHERE version = 'legacy_probe'"
                        .to_owned()
                ))
                .await
                .map_err(crate::core::Error::internal)?
                .is_some()
        );
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        database.close().await?;
        Ok(())
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[tokio::test]
    async fn one_connection_pools_with_different_prefixes_share_lock_and_history() -> Result<()> {
        use crate::database::DatabaseServiceProvider;
        use crate::testing::TestDatabase;
        use figment::providers::Serialized;

        let Ok(admin_url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping single pool migration lock: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
        let make_app = |prefix: &str| {
            let mut builder = ProviderBuilder::new();
            builder
                .provider(DatabaseServiceProvider)
                .provider(MigrationServiceProvider::<OrderedA>::new());
            let source = figment::Figment::from(Serialized::defaults(serde_json::json!({
                "app": {"env": "testing"},
                "database": {"url": database.url(), "max_connections": 1, "min_connections": 1, "table_prefix": prefix}
            })));
            async move {
                let runtime = builder.start(source).await?;
                crate::foundation::Application::from_provider_runtime(runtime)
            }
        };
        let (first, second) = tokio::try_join!(make_app("first_"), make_app("second_"))?;
        let (first_result, second_result) = tokio::join!(
            first.run_command("migrate", Vec::new()),
            second.run_command("migrate", Vec::new()),
        );
        first_result?;
        second_result?;
        assert_eq!(migration_versions(&database.connection()?).await?.len(), 2);
        first
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        second
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        database.close().await?;
        Ok(())
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    async fn migration_versions(
        connection: &crate::database::DatabaseConnection,
    ) -> Result<Vec<String>> {
        use sea_orm_migration::sea_orm::{ConnectionTrait, Statement};
        let rows = connection
            .query_all_raw(Statement::from_string(
                connection.get_database_backend(),
                "SELECT version FROM openrust_migrations".to_owned(),
            ))
            .await
            .map_err(crate::core::Error::internal)?;
        let mut versions = rows
            .into_iter()
            .map(|row| {
                row.try_get("", "version")
                    .map_err(crate::core::Error::internal)
            })
            .collect::<Result<Vec<String>>>()?;
        versions.sort();
        Ok(versions)
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn migration_actions_default_to_all_up_or_one_down() {
        assert!(matches!(
            MigrationOptions::parse(&[]),
            Ok(MigrationOptions {
                action: MigrationAction::Up(None),
                database: None,
                ..
            })
        ));
        assert!(matches!(
            MigrationOptions::parse(&args(&["up"])),
            Ok(MigrationOptions {
                action: MigrationAction::Up(None),
                database: None,
                ..
            })
        ));
        assert!(matches!(
            MigrationOptions::parse(&args(&["down"])),
            Ok(MigrationOptions {
                action: MigrationAction::Down(None),
                database: None,
                ..
            })
        ));
        assert!(matches!(
            MigrationOptions::parse(&args(&["--step"])),
            Ok(MigrationOptions {
                action: MigrationAction::UpStep,
                database: None,
                ..
            })
        ));
    }

    #[test]
    fn migration_actions_accept_positive_step_counts_and_status() {
        assert!(matches!(
            MigrationOptions::parse(&args(&["up", "2"])),
            Ok(MigrationOptions {
                action: MigrationAction::Up(Some(2)),
                database: None,
                ..
            })
        ));
        assert!(matches!(
            MigrationOptions::parse(&args(&["down", "3"])),
            Ok(MigrationOptions {
                action: MigrationAction::Down(Some(3)),
                database: None,
                ..
            })
        ));
        assert!(matches!(
            MigrationOptions::parse(&args(&["status"])),
            Ok(MigrationOptions {
                action: MigrationAction::Status,
                database: None,
                ..
            })
        ));
    }

    #[test]
    fn migration_action_selects_an_explicit_database() {
        assert!(matches!(
            MigrationOptions::parse(&args(&["--database", "analytics", "up", "2"])),
            Ok(MigrationOptions { database: Some(name), action: MigrationAction::Up(Some(2)), .. }) if name == "analytics"
        ));
        assert!(MigrationOptions::parse(&args(&["--database"])).is_err());
        assert!(
            MigrationOptions::parse(&args(&["--database", "analytics", "--database", "other"]))
                .is_err()
        );
    }

    #[test]
    fn production_confirmation_is_required_only_for_unapproved_writes() -> Result<()> {
        for args in [&["up", "-n"][..], &["down", "--no-interaction", "2"]] {
            let options = MigrationOptions::parse(&self::args(args))?;
            assert!(options.authorize(false, false).is_ok());
            assert!(options.authorize(true, true).is_ok());
            assert!(matches!(
                options.authorize(true, false),
                Err(error) if error.code() == "migration_confirmation_required"
            ));
        }
        let forced =
            MigrationOptions::parse(&args(&["down", "--force", "--database=analytics", "2"]))?;
        assert_eq!(forced.database.as_deref(), Some("analytics"));
        assert!(forced.authorize(true, false).is_ok());
        let status = MigrationOptions::parse(&args(&["status", "--no-interaction"]))?;
        assert!(status.authorize(true, false).is_ok());
        Ok(())
    }

    #[test]
    fn invalid_migration_arguments_are_rejected() {
        for invalid in [
            &["fresh"][..],
            &["down", "0"],
            &["up", "-1"],
            &["status", "1"],
            &["status", "--force"],
            &["--database", "--force"],
            &["--database="],
        ] {
            assert!(MigrationOptions::parse(&args(invalid)).is_err());
        }
    }
}
