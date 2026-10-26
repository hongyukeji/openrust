//! 针对所选数据库 backend 验证运行时迁移命令。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, StatusCode};
use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::core::RateLimiter;
use openrust::database::{
    DatabaseConfig, DatabaseRateLimitMigration, DatabaseRateLimitStore, DatabaseTableNames, Db,
    MigrationPlugin, SeedPlugin, dialect, sea_orm, sea_orm_migration,
};
use openrust::http::{AppBuilderHttpExt, HttpPlugin, RateLimitLayer, axum, build_router};
#[cfg(feature = "queue")]
use openrust::queue::QueueMigration;
use openrust::{
    App, AppBuilder, BoxFuture, Error, Plugin, Result as FrameworkResult, config::FrameworkConfig,
    lifecycle,
};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
};
use sea_orm_migration::prelude::*;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::ServiceExt;

static SEED_RUNS: AtomicUsize = AtomicUsize::new(0);

struct CustomConnectionPlugin;

impl Plugin for CustomConnectionPlugin {
    fn build(&self, app: &mut AppBuilder) -> FrameworkResult<()> {
        if app.get::<Db>().is_some() {
            return Err(Error::custom(
                "database_service_already_registered",
                "register only one database plugin per application",
            ));
        }
        let config = app.section::<DatabaseConfig>("database")?;
        let table_names =
            DatabaseTableNames::new(config.table_prefix.clone(), config.table_names.clone())?;
        let db = Db::with_table_names(table_names);
        app.insert(config);
        app.insert(RateLimiter::new(DatabaseRateLimitStore::new(db.clone())));
        app.insert(db);
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, FrameworkResult<()>> {
        Box::pin(async move {
            let config = app.state().try_get::<DatabaseConfig>()?;
            let url = config
                .url
                .as_ref()
                .ok_or_else(|| Error::custom("missing_database_url", "database.url is required"))?;
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
            app.state().try_get::<Db>()?.initialize(connection)
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
        let mut migrations: Vec<Box<dyn MigrationTrait>> =
            vec![Box::new(CreateProbe), Box::new(DatabaseRateLimitMigration)];
        #[cfg(feature = "queue")]
        migrations.push(Box::new(QueueMigration));
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
        DbErr::Migration("probe migration requires MigrationPlugin table-name scope".into())
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
    let test_database = testing::TestDatabase::create::<TestMigrator>(&url).await?;
    let source = format!("[database]\nurl = {:?}\n", test_database.url());
    let figment = Figment::new().merge(Toml::string(&source));
    let mut framework_config = FrameworkConfig::default();
    framework_config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let mut builder = AppBuilder::new(framework_config, figment);
    builder
        .plugin(CustomConnectionPlugin)
        .plugin(HttpPlugin)
        .plugin(MigrationPlugin::<TestMigrator>::new())
        .plugin(SeedPlugin::new(seed_probe))
        .routes(|routes| {
            routes.get("/limited", migration_limit_probe).layer(
                RateLimitLayer::by_client_ip("migration:probe", 1, Duration::from_secs(60)).layer(),
            )
        });

    let app = lifecycle::finalize(builder).await?;
    assert!(app.state().try_get::<RateLimiter>().is_ok());
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

    let database = app.state().try_get::<Db>()?.connection()?;
    let db_service = app.state().try_get::<Db>()?;
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

    let limiter = app.state().try_get::<RateLimiter>()?;
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
