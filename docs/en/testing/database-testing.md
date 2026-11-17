# Database Testing

## Isolating the database

Use `TestApp::sqlite` to give each test application its own SQLite file. During provider registration the fixture replaces the configured connections and selects SQLite. The file lives under the test root, so tests can run in parallel with independent data.

## Migrations and seeders

Register application migrations and seeders in the builder callback as you would in the application. Then run `test.artisan(["migrate", "--seed"]).await.assertSuccessful()`. To rebuild the schema and seed again in the same fixture, use `migrate:fresh --seed`. Each new fixture starts with an empty database file. Each database file provides a separate test boundary.

## Managed MySQL test schemas {#managed-mysql-test-schemas}

Use `TestApp::mysql` to allocate a private schema on a dedicated MySQL test server. Register migrations and seeders in the builder, then run them through Artisan. The account needs permission to create and drop test schemas, install an ownership routine, and read the server activity tables in `performance_schema`.

```rust
use openrust::testing::TestApp;

async fn run_migrations(
    admin_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::mysql(admin_url, |builder| builder).await?;
    test.artisan(["migrate"]).await.assertSuccessful();
    test.close().await
}
```

Call `close().await` to wait for cleanup. It closes application pools and deletes the schema only after verifying the management lease, ownership marker, and absence of other connected sessions. If a check fails, the error reports the schema name for inspection. A dropped fixture attempts cleanup while its Tokio runtime is running.

## Refreshing the database {#refreshing-the-database}

Use `refreshDatabase` with a managed MySQL fixture. It runs `migrate:fresh` before the first callback and reuses the migrated baseline for later callbacks with the same options. Each callback runs in its own rollback transaction. A named seeder takes precedence over `seed`.

```rust
use openrust::testing::{RefreshDatabaseOptions, TestApp};

async fn check_baseline(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = RefreshDatabaseOptions { seed: true, ..Default::default() };
    test.refreshDatabase(options, |test| Box::pin(async move {
        test.assertDatabaseCount("items", 1).await
    })).await
}
```

A migration or Seeder failure returns before the callback and leaves the fixture ready to retry preparation. A callback error with a successful rollback preserves the baseline. An interrupted transaction or changed options rebuilds it on the next call. If a caller is cancelled during migration, the next refresh waits for that migration to finish first.

## MySQL transaction isolation {#mysql-transaction-isolation}

Configure the test application to use a dedicated MySQL test database. Run migrations and baseline seeders before calling `test.databaseTransaction`. Inside the callback, HTTP requests, in-process Artisan commands, and database assertions on the default connection share one session. The helper rolls back on success or error, and application transactions use savepoints.

```rust
use openrust::testing::TestApp;

async fn check_seeded_items(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.databaseTransaction(|test| Box::pin(async move {
        test.artisan(["db:seed"]).await.assertSuccessful();
        test.assertDatabaseHas("items", &[("name", "seeded".into())]).await
    })).await
}
```

Use InnoDB tables. Run schema changes outside the callback. Await each operation on the session before starting another. Spawned tasks, external processes, and other named connections run outside the callback’s transaction.

## Database assertions

Use `assertDatabaseHas` and `assertDatabaseMissing` to check row values, including nulls. Use `assertDatabaseCount` to check the row count.

```rust
use openrust::{database::query::Record, testing::TestApp};

#[tokio::test]
async fn keeps_records_in_the_test_database() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::sqlite(|builder| builder)?;
    let db = test.database().await?;
    db.getSchemaBuilder()
        .create("items", |table| {
            table.id();
            table.string("name", 40);
        })
        .await?;
    db.table("items")
        .insert(&[Record::from([("name".into(), "fixture".into())])])
        .await?;
    test.assertDatabaseHas("items", &[("name", "fixture".into())]).await?;
    test.assertDatabaseCount("items", 1).await?;
    Ok(())
}
```
