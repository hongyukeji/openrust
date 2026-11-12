use std::{error::Error, sync::Arc};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        DatabaseFuture, DatabaseResult, Value,
        migrations::{Migration, MigrationContext, MigrationEntry, MigrationRegistry},
        query::Record,
        seeder::{Seeder, SeederContext, SeederEntry, SeederRegistry},
    },
    foundation::{Application, configuration::ApplicationBuilder},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
    testing::TestApp,
};
use serde_json::json;

struct CreateItems;

impl Migration for CreateItems {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .getSchemaBuilder()
                .create("items", |table| {
                    table.id();
                    table.string("name", 40);
                    table.string("description", 80).nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move { context.getSchemaBuilder().dropIfExists("items").await })
    }
}

struct SeedItems;

impl Seeder for SeedItems {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .connection()
                .table("items")
                .insert(&[Record::from([("name".into(), "seeded".into())])])
                .await?;
            Ok(())
        })
    }
}

struct InventoryProvider;

impl ServiceProvider for InventoryProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
                "database/migrations/0001_create_items.rs",
                &CreateItems,
            )];
            static SEEDERS: &[SeederEntry] = &[SeederEntry::new("DatabaseSeeder", &SeedItems)];
            app.container()
                .instance(Arc::new(MigrationRegistry(MIGRATIONS)))?;
            app.container()
                .instance(Arc::new(SeederRegistry(SEEDERS)))?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn migrations_and_seeders_only_change_their_own_sqlite_fixture()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let configure = |builder: ApplicationBuilder| {
        builder
            .withConfig(|_, _| {
                Ok([
                    ("app".into(), json!({"env": "testing"})),
                    (
                        "database".into(),
                        json!({
                            "default": "external",
                            "connections": {"external": {"driver": "unavailable"}},
                            "migrations": {"table": "test_migrations"},
                        }),
                    ),
                ]
                .into())
            })
            .withProviders(vec![Box::new(InventoryProvider)])
    };

    let mut first = TestApp::sqlite(configure)?;
    let mut second = TestApp::sqlite(configure)?;
    let first_root = first.root().to_owned();
    let second_root = second.root().to_owned();
    assert_ne!(first_root, second_root);

    let (first_result, second_result) = tokio::join!(
        first.artisan(["migrate", "--seed"]),
        second.artisan(["migrate", "--seed"]),
    );
    first_result.assertSuccessful();
    second_result.assertSuccessful();
    let config = first.app().config()?;
    assert_eq!(config.get("database.default"), Some(json!("sqlite")));
    assert!(config.get("database.connections.external").is_none());
    assert_eq!(
        config.get("database.migrations.table"),
        Some(json!("test_migrations"))
    );

    first
        .assertDatabaseHas(
            "items",
            &[("name", "seeded".into()), ("description", Value::Null)],
        )
        .await?;
    first.assertDatabaseCount("test_migrations", 1).await?;
    first
        .database()
        .await?
        .table("items")
        .insert(&[Record::from([("name".into(), "first only".into())])])
        .await?;
    second
        .assertDatabaseMissing("items", &[("name", "first only".into())])
        .await?;

    let invalid = second
        .artisan(["migrate:fresh", "--seed", "--seeder", "MissingSeeder"])
        .await;
    invalid.assertExitCode(1);
    assert!(invalid.stderr.contains("MissingSeeder"));
    second
        .artisan(["migrate:fresh", "--seed"])
        .await
        .assertSuccessful();
    second.assertDatabaseCount("items", 1).await?;
    first.assertDatabaseCount("items", 2).await?;

    drop(first);
    assert!(!first_root.exists());
    assert!(second_root.join("testing.sqlite").is_file());
    drop(second);
    assert!(!second_root.exists());
    Ok(())
}
