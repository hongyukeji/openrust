//! Exercises the runtime migration command against the selected database backend.
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust_core::{
    App, AppBuilder, BoxFuture, Error, Plugin, Result as FrameworkResult, config::FrameworkConfig,
    lifecycle,
};
use openrust_db::{
    DatabaseConfig, Db, MigrationPlugin, SeedPlugin, dialect, sea_orm, sea_orm_migration,
};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DbBackend, Statement};
use sea_orm_migration::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

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
        app.insert(config);
        app.insert(Db::default());
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

async fn seed_probe(_database: Db) -> openrust_core::Result<()> {
    SEED_RUNS.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

struct TestMigrator;

impl MigratorTrait for TestMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(CreateProbe)]
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
                    .table(OpenrustMigrationProbe::Table)
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
                    .table(OpenrustMigrationProbe::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
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
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder
        .plugin(CustomConnectionPlugin)
        .plugin(MigrationPlugin::<TestMigrator>::new())
        .plugin(SeedPlugin::new(seed_probe));

    let app = lifecycle::finalize(builder).await?;
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
            "SELECT COUNT(*) AS row_count FROM openrust_migration_probe".to_owned(),
        ))
        .await?
        .ok_or("migration-created table query returned no row")?;
    assert_eq!(row.try_get::<i64>("", "row_count")?, 0);

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
            "SELECT {type_column} AS column_type FROM information_schema.columns WHERE {schema_filter} AND table_name = 'openrust_migration_probe' AND column_name = '{column}'"
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
    Ok(())
}
