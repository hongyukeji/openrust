//! 针对所选数据库 backend 验证运行时迁移命令。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, StatusCode};
use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::core::RateLimiter;
use openrust::database::{
    DatabaseConfig, DatabaseRateLimitMigration, DatabaseRateLimitStore, DatabaseServiceProvider,
    DatabaseTableNames, Databases, Db, MigrationServiceProvider, SeedServiceProvider, dialect,
    sea_orm, sea_orm_migration,
};
use openrust::http::{HttpServiceProvider, RateLimitLayer, axum, build_router};
#[cfg(feature = "queue")]
use openrust::queue::QueueMigration;
use openrust::testing::TestDatabase;
use openrust::{App, Error, Result as FrameworkResult, config::FrameworkConfig};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
};
use sea_orm_migration::prelude::*;
use sha2::{Digest, Sha256};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

static SEED_RUNS: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "auth-oidc")]
#[tokio::test]
async fn framework_oidc_migrations_roundtrip_on_isolated_database()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping framework OIDC migrations: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<openrust::db::FrameworkMigrator>(&url).await?;
    let connection = database.connection()?;
    DatabaseTableNames::default()
        .scope(async {
            openrust::db::FrameworkMigrator::down(&connection, None).await?;
            openrust::db::FrameworkMigrator::up(&connection, None).await
        })
        .await?;
    let backend = connection.get_database_backend();
    let table = DatabaseTableNames::default().quote("external_identity_requests", backend)?;
    let sql = format!("SELECT 1 FROM {table} LIMIT 1");
    connection
        .query_one_raw(Statement::from_string(backend, sql))
        .await?;
    drop(connection);
    database.close().await?;
    Ok(())
}

struct CustomConnectionProvider {
    lock_url: Option<String>,
}

impl openrust::core::ServiceProvider for CustomConnectionProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, FrameworkResult<()>> {
        Box::pin(async move {
            let lock_url = self.lock_url.clone();
            app.merge_defaults(
                "database",
                serde_json::to_value(DatabaseConfig::default()).map_err(Error::internal)?,
            )?;
            app.singleton::<Db, _>(move |resolver| {
                let lock_url = lock_url.clone();
                Box::pin(async move {
                    let config = resolver.section::<DatabaseConfig>("database")?;
                    let names = DatabaseTableNames::new(
                        config.table_prefix.clone(),
                        config.table_names.clone(),
                    )?;
                    let db = Db::with_table_names(names);
                    let url = config.url.as_ref().ok_or_else(|| {
                        Error::custom("missing_database_url", "database.url is required")
                    })?;
                    let mysql = url.starts_with("mysql:");
                    #[cfg(feature = "db-mysql")]
                    let mut options = ConnectOptions::new(url.clone());
                    #[cfg(not(feature = "db-mysql"))]
                    let options = ConnectOptions::new(url.clone());
                    if mysql {
                        #[cfg(feature = "db-mysql")]
                        {
                            options.map_sqlx_mysql_opts(|mysql| {
                                mysql.ssl_mode(sqlx::mysql::MySqlSslMode::Required)
                            });
                            options.after_connect(|connection| {
                                Box::pin(async move {
                                    connection
                                        .execute_unprepared("SET time_zone = '+00:00'")
                                        .await?;
                                    Ok(())
                                })
                            });
                        }
                    }
                    let connection = Database::connect(options).await.map_err(Error::internal)?;
                    let closing = connection.clone();
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(
                            async move { closing.close_by_ref().await.map_err(Error::internal) },
                        )
                    })?;
                    db.initialize(connection)?;
                    db.configure_migration_lock_url(lock_url.unwrap_or_else(|| url.clone()))?;
                    Ok(std::sync::Arc::new(db))
                })
            })
            .await?;
            app.singleton::<RateLimiter, _>(|resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    Ok(std::sync::Arc::new(RateLimiter::new(
                        DatabaseRateLimitStore::new((*db).clone()),
                    )))
                })
            })
            .await?;
            Ok(())
        })
    }
}

async fn seed_probe(_database: Db) -> openrust::Result<()> {
    SEED_RUNS.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

struct TestMigrator;

impl MigratorTrait for TestMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let migrations: Vec<Box<dyn MigrationTrait>> =
            vec![Box::new(CreateProbe), Box::new(DatabaseRateLimitMigration)];
        #[cfg(feature = "queue")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(QueueMigration));
            migrations
        };
        migrations
    }
}

struct CreateProbe;

impl MigrationName for CreateProbe {
    fn name(&self) -> &'static str {
        "m20261025_000001_create_openrust_migration_probe"
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateProbe {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(probe_table_name()?)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(OpenrustMigrationProbe::Id)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(dialect::timestamp_utc(
                        OpenrustMigrationProbe::CreatedAt,
                        manager.get_database_backend(),
                    )?)
                    .col(dialect::json(
                        OpenrustMigrationProbe::Payload,
                        manager.get_database_backend(),
                    )?)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(probe_table_name()?)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

fn probe_table_name() -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "probe migration requires MigrationServiceProvider table-name scope".into(),
        )
    })?;
    names
        .resolve("openrust_migration_probe")
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

#[derive(DeriveIden)]
#[allow(dead_code)]
enum OpenrustMigrationProbe {
    Table,
    Id,
    CreatedAt,
    Payload,
}

#[tokio::test]
async fn migration_command_applies_repeats_reports_and_rolls_back()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database migration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let test_database = openrust::testing::TestDatabase::create::<EmptyMigrator>(&url).await?;
    let source = format!("[database]\nurl = {:?}\n", test_database.url());
    let figment = Figment::new().merge(Toml::string(&source));
    let mut framework_config = FrameworkConfig::default();
    framework_config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let sources =
        figment::Figment::from(figment::providers::Serialized::defaults(framework_config))
            .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(CustomConnectionProvider { lock_url: None })
        .provider(HttpServiceProvider)
        .provider(MigrationServiceProvider::<TestMigrator>::new())
        .provider(SeedServiceProvider::new(seed_probe))
        .provider(TestRoutes::new(|routes| {
            routes.get("/limited", migration_limit_probe).layer(
                RateLimitLayer::by_client_ip("migration:probe", 1, Duration::from_secs(60)).layer(),
            )
        }));

    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    assert!(app.state().resolve::<RateLimiter>().await.is_ok());
    app.run_command("migrate", vec![]).await?;
    app.run_command("migrate", vec!["up".into()]).await?;
    app.run_command("migrate", vec!["status".into()]).await?;
    app.run_command("db", vec!["seed".into()]).await?;
    assert_eq!(SEED_RUNS.load(Ordering::SeqCst), 1);
    assert!(
        app.run_command("db", vec!["seed".into(), "unexpected".into()])
            .await
            .is_err()
    );

    let database = app.state().resolve::<Db>().await?.connection()?;
    let db_service = app.state().resolve::<Db>().await?;
    assert_eq!(db_service.backend()?, database.get_database_backend());
    assert!(db_service.initialize(database.clone()).is_err());
    let row = database
        .query_one_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "SELECT COUNT(*) AS row_count FROM {}",
                db_service.quoted_table_name("openrust_migration_probe")?
            ),
        ))
        .await?
        .ok_or("migration-created table query returned no row")?;
    assert_eq!(row.try_get::<i64>("", "row_count")?, 0);

    let limiter = app.state().resolve::<RateLimiter>().await?;
    verify_rate_limiter(&database, &db_service, &limiter).await?;
    verify_http_rate_limit_layer(&app).await?;

    let postgres = database.get_database_backend() == DbBackend::Postgres;
    for (column, expected_type) in [
        (
            "created_at",
            if postgres {
                "timestamp with time zone"
            } else {
                "datetime(6)"
            },
        ),
        ("payload", if postgres { "jsonb" } else { "json" }),
    ] {
        let (schema_filter, type_column) = if postgres {
            ("table_schema = current_schema()", "data_type")
        } else {
            ("table_schema = DATABASE()", "column_type")
        };
        let query = format!(
            "SELECT {type_column} AS column_type FROM information_schema.columns WHERE {schema_filter} AND table_name = '{}' AND column_name = '{column}'",
            db_service.table_name("openrust_migration_probe")?
        );
        let row = database
            .query_one_raw(Statement::from_string(
                database.get_database_backend(),
                query,
            ))
            .await?
            .ok_or("migration column metadata query returned no row")?;
        assert_eq!(row.try_get::<String>("", "column_type")?, expected_type);
    }

    app.run_command("migrate", vec!["down".into()]).await?;
    app.run_command("migrate", vec!["up".into()]).await?;
    app.run_command("migrate", vec!["down".into()]).await?;
    drop(limiter);
    drop(database);
    drop(db_service);
    drop(app);
    test_database.close().await?;
    Ok(())
}

struct EmptyMigrator;

impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

struct LockProbeMigrator;

impl MigratorTrait for LockProbeMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(LockProbeMigration)]
    }
}

struct LockProbeMigration;

impl MigrationName for LockProbeMigration {
    fn name(&self) -> &'static str {
        "m20261027_000001_migration_lock_probe"
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for LockProbeMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if let Ok(path) = std::env::var("OPENRUST_MIGRATION_ACTIVE_FILE") {
            std::fs::write(path, b"active").map_err(|error| DbErr::Migration(error.to_string()))?;
        }
        if let Ok(delay) = std::env::var("OPENRUST_MIGRATION_DELAY_MS")
            && let Ok(delay) = delay.parse::<u64>()
        {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let names = DatabaseTableNames::current().ok_or_else(|| {
            DbErr::Migration("migration lock probe requires a table-name scope".into())
        })?;
        manager
            .create_table(
                Table::create()
                    .table(Alias::new(
                        names
                            .resolve("migration_lock_probe")
                            .map_err(|error| DbErr::Migration(error.to_string()))?,
                    ))
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .small_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let names = DatabaseTableNames::current().ok_or_else(|| {
            DbErr::Migration("migration lock probe requires a table-name scope".into())
        })?;
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new(
                        names
                            .resolve("migration_lock_probe")
                            .map_err(|error| DbErr::Migration(error.to_string()))?,
                    ))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

async fn build_lock_probe_app(url: &str, timeout: Duration) -> FrameworkResult<App> {
    let source = format!("[database]\nurl = {url:?}\ntable_prefix = 'migration_lock_test_'\n");
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Figment::new().merge(Toml::string(&source)));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider).provider(
        MigrationServiceProvider::<LockProbeMigrator>::new().with_lock_wait_timeout(timeout),
    );
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

#[tokio::test]
async fn custom_database_rejects_lock_connection_to_another_database()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!(
            "skipping migration lock identity integration: OPENRUST_TEST_DATABASE_URL is unset"
        );
        return Ok(());
    };
    let primary = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let other = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let source = format!("[database]\nurl = {:?}\n", primary.url());
    let sources = Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Toml::string(&source));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(CustomConnectionProvider {
            lock_url: Some(other.url().to_owned()),
        })
        .provider(MigrationServiceProvider::<LockProbeMigrator>::new());
    let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
    let error = app
        .run_command("migrate", vec!["up".into()])
        .await
        .err()
        .ok_or("different database lock connection unexpectedly succeeded")?;
    assert_eq!(error.code(), "migration_lock_database_mismatch");
    let primary_connection = app.state().resolve::<Db>().await?.connection()?;
    let schema_filter = if primary_connection.get_database_backend() == DbBackend::Postgres {
        "table_schema = current_schema()"
    } else {
        "table_schema = DATABASE()"
    };
    let probe = primary_connection
        .query_one_raw(Statement::from_string(
            primary_connection.get_database_backend(),
            format!("SELECT COUNT(*) AS count FROM information_schema.tables WHERE {schema_filter} AND table_name = 'openrust_migrations'"),
        ))
        .await?
        .ok_or("migration history probe did not return a row")?;
    assert_eq!(probe.try_get::<i64>("", "count")?, 0);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    primary.close().await?;
    other.close().await?;
    Ok(())
}

fn spawn_lock_probe_migration(
    url: &str,
    active_file: &str,
    delay_ms: u64,
) -> std::io::Result<Child> {
    Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "child_migration_process_runs_migrations",
            "--nocapture",
        ])
        .env("OPENRUST_MIGRATION_CHILD_URL", url)
        .env("OPENRUST_MIGRATION_ACTIVE_FILE", active_file)
        .env("OPENRUST_MIGRATION_DELAY_MS", delay_ms.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

async fn wait_for_file(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !std::path::Path::new(path).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

fn unique_migration_probe_path(label: &str) -> Result<String, Box<dyn std::error::Error>> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(std::env::temp_dir()
        .join(format!(
            "openrust-migration-{label}-{}-{stamp}",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned())
}

#[tokio::test]
async fn migration_commands_serialize_timeout_and_recover_after_process_exit()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping migration lock integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let isolated = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let app = build_lock_probe_app(isolated.url(), Duration::from_millis(150)).await?;

    let first_path = unique_migration_probe_path("first")?;
    let mut first = spawn_lock_probe_migration(isolated.url(), &first_path, 800)?;
    wait_for_file(&first_path).await?;
    let second_path = unique_migration_probe_path("second")?;
    let mut second = spawn_lock_probe_migration(isolated.url(), &second_path, 0)?;
    let timeout = app.run_command("migrate", vec!["status".into()]).await;
    assert_eq!(
        timeout.err().map(|error| error.code().to_owned()),
        Some("migration_lock_timeout".to_owned())
    );
    let first_status = tokio::task::spawn_blocking(move || first.wait()).await??;
    assert!(first_status.success());
    let second_status = tokio::task::spawn_blocking(move || second.wait()).await??;
    assert!(second_status.success());
    assert!(!std::path::Path::new(&second_path).exists());
    std::fs::remove_file(&first_path)?;

    app.run_command("migrate", vec!["status".into()]).await?;
    app.run_command("migrate", vec!["down".into()]).await?;
    let crash_path = unique_migration_probe_path("crash")?;
    let mut crashed = spawn_lock_probe_migration(isolated.url(), &crash_path, 10_000)?;
    wait_for_file(&crash_path).await?;
    crashed.kill()?;
    let crash_status = tokio::task::spawn_blocking(move || crashed.wait()).await??;
    assert!(!crash_status.success());
    std::fs::remove_file(&crash_path)?;
    app.run_command("migrate", vec!["up".into()]).await?;
    app.run_command("migrate", vec!["down".into()]).await?;

    drop(app);
    isolated.close().await?;
    Ok(())
}

#[tokio::test]
async fn child_migration_process_runs_migrations() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_MIGRATION_CHILD_URL") else {
        return Ok(());
    };
    let app = build_lock_probe_app(&url, Duration::from_secs(10)).await?;
    app.run_command("migrate", vec!["up".into()]).await?;
    Ok(())
}

#[tokio::test]
async fn named_database_instances_keep_migrations_and_transactions_isolated()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping named database integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let default_database =
        openrust::testing::TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let named_database =
        openrust::testing::TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let reporting_database =
        openrust::testing::TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let source = format!(
        "[database]\nurl = {:?}\nread_replicas = [{:?}]\n[database.instances.analytics]\nurl = {:?}\nread_replicas = [{:?}]\ntable_prefix = 'analytics_'\n[database.instances.reporting]\nurl = {:?}\nread_replicas = [{:?}]\ntable_prefix = 'reporting_'\n",
        default_database.url(),
        default_database.url(),
        named_database.url(),
        named_database.url(),
        reporting_database.url(),
        reporting_database.url()
    );
    let figment = Figment::new().merge(Toml::string(&source));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(MigrationServiceProvider::<TestMigrator>::new());
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let databases = app.state().resolve::<Databases>().await?;
    let default_db = app.state().resolve::<Db>().await?;
    let analytics_db = databases.get("analytics")?;
    let reporting_db = databases.get("reporting")?;
    assert_named_database_isolation(&databases, &default_db, &analytics_db, &reporting_db).await?;

    app.run_command(
        "migrate",
        vec!["--database".into(), "analytics".into(), "up".into()],
    )
    .await?;
    assert_eq!(
        analytics_db.table_name("openrust_migration_probe")?,
        "analytics_openrust_migration_probe"
    );
    assert_eq!(
        default_db.table_name("openrust_migration_probe")?,
        "tb_openrust_migration_probe"
    );
    assert_eq!(
        reporting_db.table_name("openrust_migration_probe")?,
        "reporting_openrust_migration_probe"
    );
    assert_probe_row_count(&analytics_db, 0).await?;
    assert!(assert_probe_row_count(&default_db, 0).await.is_err());
    app.run_command("migrate", vec!["up".into()]).await?;
    assert_probe_row_count(&default_db, 0).await?;
    insert_probe_row(&default_db, 1).await?;
    insert_probe_row(&analytics_db, 1).await?;
    assert_probe_row_count(&default_db, 1).await?;
    assert_probe_row_count(&analytics_db, 1).await?;
    assert!(assert_probe_row_count(&reporting_db, 0).await.is_err());

    default_db.connection()?.close().await?;
    analytics_db.connection()?.close().await?;
    reporting_db.connection()?.close().await?;
    drop(app);
    drop(analytics_db);
    drop(reporting_db);
    drop(default_db);
    drop(databases);
    named_database.close().await?;
    reporting_database.close().await?;
    default_database.close().await?;
    Ok(())
}

async fn assert_named_database_isolation(
    databases: &Databases,
    default_db: &Db,
    analytics_db: &Db,
    reporting_db: &Db,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        databases.names(),
        vec![
            "default".to_owned(),
            "analytics".to_owned(),
            "reporting".to_owned()
        ]
    );
    assert!(databases.get("missing").is_err());
    assert_eq!(
        database_identity(&default_db.connection()?).await?,
        database_identity(&databases.default_database().connection()?).await?
    );
    assert_ne!(
        database_identity(&default_db.connection()?).await?,
        database_identity(&analytics_db.connection()?).await?
    );
    assert_ne!(
        database_identity(&analytics_db.connection()?).await?,
        database_identity(&reporting_db.connection()?).await?
    );
    assert_eq!(
        database_identity(&default_db.read_connection()?).await?,
        database_identity(&default_db.connection()?).await?
    );
    assert_eq!(
        database_identity(&analytics_db.read_connection()?).await?,
        database_identity(&analytics_db.connection()?).await?
    );
    Ok(())
}

async fn assert_probe_row_count(db: &Db, expected: i64) -> openrust::Result<()> {
    let connection = db.connection()?;
    let statement = Statement::from_string(
        connection.get_database_backend(),
        format!(
            "SELECT COUNT(*) AS row_count FROM {}",
            db.quoted_table_name("openrust_migration_probe")?
        ),
    );
    let row = connection
        .query_one_raw(statement)
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::internal(std::io::Error::other("probe count returned no row")))?;
    assert_eq!(
        row.try_get::<i64>("", "row_count")
            .map_err(Error::internal)?,
        expected
    );
    Ok(())
}

async fn database_identity(connection: &DatabaseConnection) -> openrust::Result<String> {
    let (backend, query) = match connection.get_database_backend() {
        DbBackend::Postgres => (
            DbBackend::Postgres,
            "SELECT current_database() || ':' || COALESCE(inet_server_addr()::text, 'local') AS identity",
        ),
        DbBackend::MySql => (
            DbBackend::MySql,
            "SELECT CONCAT(DATABASE(), ':', @@server_uuid) AS identity",
        ),
        other => {
            return Err(Error::internal(std::io::Error::other(format!(
                "unexpected database backend: {other:?}"
            ))));
        }
    };
    let row = connection
        .query_one_raw(Statement::from_string(backend, query.to_owned()))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| {
            Error::internal(std::io::Error::other("database identity returned no row"))
        })?;
    row.try_get::<String>("", "identity")
        .map_err(Error::internal)
}

async fn insert_probe_row(db: &Db, id: i64) -> openrust::Result<()> {
    let table = db.quoted_table_name("openrust_migration_probe")?;
    let sql = format!(
        "INSERT INTO {table} (id, created_at, payload) VALUES ({id}, CURRENT_TIMESTAMP, '{{\"ok\":true}}')"
    );
    db.transaction(|transaction| {
        Box::pin(async move {
            transaction
                .execute_unprepared(&sql)
                .await
                .map(|_| ())
                .map_err(Error::internal)
        })
    })
    .await
}

async fn migration_limit_probe() -> &'static str {
    "ok"
}

async fn verify_http_rate_limit_layer(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let peer: std::net::SocketAddr = "192.0.2.10:443".parse()?;
    let router = build_router(app)?;
    for expected in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS] {
        let mut request = Request::get("/limited").body(Body::empty())?;
        request.headers_mut().insert(
            "x-forwarded-for",
            "198.51.100.42".parse().map_err(std::io::Error::other)?,
        );
        request.extensions_mut().insert(ConnectInfo(peer));
        let response = router.clone().oneshot(request).await?;
        assert_eq!(response.status(), expected);
        if expected == StatusCode::TOO_MANY_REQUESTS {
            assert!(response.headers().contains_key("retry-after"));
        }
    }
    Ok(())
}

async fn verify_rate_limiter(
    database: &DatabaseConnection,
    db_service: &Db,
    limiter: &RateLimiter,
) -> Result<(), Box<dyn std::error::Error>> {
    // 独立 service/store 实例通过数据库行竞争，模拟不同节点之间的行为。
    let second_node = RateLimiter::new(DatabaseRateLimitStore::new(db_service.clone()));
    let mut requests = tokio::task::JoinSet::new();
    for index in 0..12 {
        let limiter = if index % 2 == 0 {
            (*limiter).clone()
        } else {
            second_node.clone()
        };
        requests.spawn(async move {
            limiter
                .check(
                    "integration:one-client",
                    4,
                    std::time::Duration::from_secs(60),
                )
                .await
                .is_ok()
        });
    }
    let mut accepted = 0;
    while let Some(result) = requests.join_next().await {
        accepted += u64::from(result?);
    }
    assert_eq!(
        accepted, 4,
        "database limiter must atomically share one quota"
    );

    let key_hash = format!("{:x}", Sha256::digest(b"integration:one-client"));
    let rate_limits_table = db_service.quoted_table_name("rate_limits")?;
    let limiter_row = database
        .query_one_raw(Statement::from_string(
            database.get_database_backend(),
            format!("SELECT hit_count FROM {rate_limits_table} WHERE key_hash = '{key_hash}'"),
        ))
        .await?
        .ok_or("rate-limit row was not persisted")?;
    assert_eq!(limiter_row.try_get::<i64>("", "hit_count")?, 12);
    let plaintext_rows = database
        .query_one_raw(Statement::from_string(
            database.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {rate_limits_table} WHERE key_hash = 'integration:one-client'"),
        ))
        .await?
        .ok_or("rate-limit plaintext check returned no row")?;
    assert_eq!(plaintext_rows.try_get::<i64>("", "row_count")?, 0);

    limiter
        .check(
            "integration:window-reset",
            1,
            std::time::Duration::from_secs(1),
        )
        .await?;
    assert!(
        limiter
            .check(
                "integration:window-reset",
                1,
                std::time::Duration::from_secs(1)
            )
            .await
            .is_err()
    );
    limiter
        .check(
            "integration:cleanup-expiring",
            1,
            std::time::Duration::from_secs(1),
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    assert!(
        limiter
            .check(
                "integration:window-reset",
                1,
                std::time::Duration::from_secs(1)
            )
            .await
            .is_ok()
    );
    let cleanup_limiter = RateLimiter::new(DatabaseRateLimitStore::new(db_service.clone()));
    cleanup_limiter
        .check(
            "integration:cleanup-trigger",
            1,
            std::time::Duration::from_secs(60),
        )
        .await?;
    let expired_hash = format!("{:x}", Sha256::digest(b"integration:cleanup-expiring"));
    let expired_row_count = database
        .query_one_raw(Statement::from_string(
            database.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {rate_limits_table} WHERE key_hash = '{expired_hash}'"),
        ))
        .await?
        .ok_or("rate-limit cleanup check returned no row")?;
    assert_eq!(expired_row_count.try_get::<i64>("", "row_count")?, 0);
    Ok(())
}

struct TestRoutes<F>(std::sync::Arc<F>);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes))
    }
}
impl<F> openrust::core::ServiceProvider for TestRoutes<F>
where
    F: Fn(openrust::http::Routes) -> openrust::http::Routes + Send + Sync + 'static,
{
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let routes = std::sync::Arc::clone(&self.0);
        Box::pin(async move {
            app.routes(move |value| routes(value))?;
            Ok(())
        })
    }
}
