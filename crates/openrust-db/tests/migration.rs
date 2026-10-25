//! Exercises the runtime migration command against the selected database backend.
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust_core::{AppBuilder, config::FrameworkConfig, lifecycle};
use openrust_db::{DatabasePlugin, Db, MigrationPlugin, sea_orm_migration};
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

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
        .plugin(DatabasePlugin)
        .plugin(MigrationPlugin::<TestMigrator>::new());

    let app = lifecycle::finalize(builder).await?;
    app.run_command("migrate", vec![]).await?;
    app.run_command("migrate", vec!["up".into()]).await?;
    app.run_command("migrate", vec!["status".into()]).await?;

    let database = app.state().try_get::<Db>()?.connection()?;
    let row = database
        .query_one_raw(Statement::from_string(
            database.get_database_backend(),
            "SELECT COUNT(*) AS row_count FROM openrust_migration_probe".to_owned(),
        ))
        .await?
        .ok_or("migration-created table query returned no row")?;
    assert_eq!(row.try_get::<i64>("", "row_count")?, 0);

    app.run_command("migrate", vec!["down".into()]).await?;
    app.run_command("migrate", vec!["up".into()]).await?;
    app.run_command("migrate", vec!["down".into()]).await?;
    Ok(())
}
