use std::{error::Error, sync::Arc};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        DB, DatabaseFuture, DatabaseResult, Value,
        migrations::{Migration, MigrationContext, MigrationEntry, MigrationRegistry},
        query::Record,
        seeder::{Seeder, SeederContext, SeederEntry, SeederRegistry},
    },
    foundation::{
        Application,
        configuration::{ApplicationBuilder, Routing},
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider, facades::Route},
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

#[tokio::test]
async fn mysql_test_transaction_shares_http_artisan_and_nested_writes()
-> Result<(), Box<dyn Error + Send + Sync>> {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    }
    let configure = |builder: ApplicationBuilder| {
        builder
            .withConfig(|_, _| {
                let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
                Ok([(
                    "database".into(),
                    json!({
                        "default": "mysql", "connections": {"mysql": {"url": url}},
                    }),
                )]
                .into())
            })
            .withProviders(vec![Box::new(InventoryProvider)])
            .withRouting(Routing::default().web(|| {
                Route::get("/write", write_item)?;
                Ok(())
            }))
    };
    let mut test = TestApp::create(configure)?;
    let mut observer = TestApp::create(configure)?;
    test.artisan(["migrate", "--seed"]).await.assertSuccessful();
    let external = observer.database().await?;

    test.databaseTransaction(|test| {
        Box::pin(async {
            test.assertDatabaseCount("items", 1).await?;
            test.get("/write")
                .await?
                .assertStatus(StatusCode::NO_CONTENT);
            test.artisan(["db:seed"]).await.assertSuccessful();
            test.assertDatabaseHas("items", &[("name", "http".into())])
                .await?;
            let db = test.database().await?;
            let mut nested = db.beginTransaction().await?;
            nested
                .table("items")
                .insert(&[Record::from([("name".into(), "nested".into())])])
                .await?;
            nested.commit().await?;
            test.assertDatabaseCount("items", 4).await?;
            assert!(
                db.statement("CREATE TABLE must_not_exist (id INT)", &[])
                    .await
                    .is_err()
            );
            Ok(())
        })
    })
    .await?;
    assert_eq!(external.table("items").count("*").await?, 1);
    test.assertDatabaseMissing("items", &[("name", "http".into())])
        .await?;

    let error = test
        .databaseTransaction(|test| {
            Box::pin(async {
                test.artisan(["db:seed"]).await.assertSuccessful();
                Err::<(), Box<dyn Error + Send + Sync>>("callback failed".into())
            })
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "callback failed");
    assert_eq!(external.table("items").count("*").await?, 1);

    let interrupted = test
        .databaseTransaction(|test| {
            Box::pin(async {
                let db = test.database().await?;
                let mut abandoned = db.beginTransaction().await?;
                abandoned
                    .table("items")
                    .insert(&[Record::from([("name".into(), "abandoned".into())])])
                    .await?;
                drop(abandoned);
                assert!(db.table("items").count("*").await.is_err());
                Ok(())
            })
        })
        .await
        .unwrap_err();
    assert!(interrupted.to_string().contains("transaction"));
    assert_eq!(external.table("items").count("*").await?, 1);

    {
        let (written, ready) = tokio::sync::oneshot::channel();
        let test = test.databaseTransaction(|test| {
            Box::pin(async move {
                test.get("/write")
                    .await?
                    .assertStatus(StatusCode::NO_CONTENT);
                written.send(()).unwrap();
                std::future::pending::<Result<(), Box<dyn Error + Send + Sync>>>().await
            })
        });
        tokio::pin!(test);
        tokio::select! {
            result = &mut test => panic!("test transaction returned before cancellation: {result:?}"),
            result = ready => result?,
        }
    }
    assert_eq!(external.table("items").count("*").await?, 1);
    Ok(())
}

fn write_item(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        async {
            DB::connection(None)?
                .table("items")
                .insert(&[Record::from([("name".into(), "http".into())])])
                .await
        }
        .await
        .map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
