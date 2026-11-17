# Database Testing

## Isolating the database

Use `TestApp::sqlite` to give each test application its own SQLite file. During provider registration the fixture replaces the configured connections and selects SQLite. The file lives under the test root, so tests can run in parallel with independent data.

## Migrations and seeders

Register application migrations and seeders in the builder callback as you would in the application. Then run `test.artisan(["migrate", "--seed"]).await.assertSuccessful()`. To rebuild the schema and seed again in the same fixture, use `migrate:fresh --seed`. Each new fixture starts with an empty database file. Each database file provides a separate test boundary.

### Running seeders {#running-seeders}

Call `test.seed().await?` to run the registered `DatabaseSeeder`. For one or more named seeders, pass their names to `seedWith`. The list runs in order and an empty list is a no-op. Use `seedOn("reports", None)` for the default seeder on a named connection, or pass `Some(&[...])` to select specific seeders.

Each seeder runs through Artisan `db:seed` with prompts disabled. A failed command returns an error and stops the list. Inside a test transaction, the writes on each selected connection roll back with that transaction. Other connections retain their own commit behavior.

```rust
use openrust::testing::TestApp;

async fn populate(
    test: &mut TestApp,
    seeders: &[&str],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.seed().await?;
    test.seedWith(seeders).await
}
```

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

### Recovering an abandoned MySQL test schema {#recovering-mysql-test-schemas}

To recover after a test process is terminated, use `TestApp::mysqlWithRecovery` or `TestApp::mysqlConnectionsWithRecovery`. Pass an existing private directory on the local filesystem, outside the fixture’s temporary root. On Unix, use mode `0700`. Each schema gets an `openrust_test_<id>.json` receipt before creation. The receipt contains a server identifier and an ownership token. Keep it private. Connection URLs and credentials stay outside the receipt.

From a fresh process, pass the receipt path and the original dedicated test server’s management URL. Recovery checks the server UUID, acquires the schema lease, and verifies the ownership routine and server activity before dropping a schema. When a lease is held, the schema is busy, or ownership cannot be verified, recovery returns an error and keeps the receipt. Successful cleanup removes it. Repeating recovery after cleanup succeeds without taking any action.

```rust
use std::path::Path;
use openrust::testing::TestApp;

async fn recover_test_schema(admin_url: &str, receipt: &Path) -> std::io::Result<()> {
    TestApp::recoverMysql(admin_url, receipt).await
}
```

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

Set `without_migration_events: true` to mute query and other connection events on the selected connections during preparation. Migration lifecycle events, query logs, and events from other connections remain available. The test callback and commands run outside preparation dispatch connection events normally.

## Rebuilding and rolling back migrations {#database-migrations}

Use `runDatabaseMigrations` when a test needs to commit its own transactions. On the fixture’s managed default MySQL connection, it runs `migrate:fresh` before the callback and `migrate:rollback` afterward, including when the callback returns an error or panics. The rollback targets the same schema used for preparation.

Set `drop_views` to include views in the rebuild. Set `seed` to run the default seeder, or supply `seeder` to select a named one. After cancelling the future, call `close().await` or start another database reset to finish the pending rollback.

```rust
use openrust::testing::{DatabaseMigrationsOptions, TestApp};

async fn check_empty_items(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.runDatabaseMigrations(DatabaseMigrationsOptions::default(), |test| {
        Box::pin(async { test.assertDatabaseEmpty("items").await })
    }).await
}
```

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

## Multiple test connections {#multiple-test-connections}

Call `TestApp::mysqlConnections` with named server URLs to create an owned schema for each connection. The first name becomes the default. With `refreshDatabase`, `migrate:fresh` prepares the default connection. Prepare any secondary schemas with `migrate --database=<name>` before the callback.

```rust
use openrust::testing::{RefreshDatabaseOptions, TestApp};

async fn run_test(
    url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::mysqlConnections(&[("mysql", url), ("reports", url)], |builder| {
        builder
    }).await?;
    test.artisan(["migrate", "--database=reports"]).await.assertSuccessful();
    let options = RefreshDatabaseOptions {
        connections_to_transact: vec!["mysql".into(), "reports".into()],
        ..Default::default()
    };
    test.refreshDatabase(options, |_| Box::pin(async { Ok(()) })).await?;
    test.close().await
}
```

Set `connections_to_transact` to select the connections that roll back after each callback. An empty list selects the default. For an already prepared fixture, `databaseTransactionOn` runs the same rollback lifecycle without a schema refresh.

## Database assertions

Use `assertDatabaseHas` and `assertDatabaseMissing` to check row values, including nulls. Use `assertDatabaseCount` to check the row count and `assertDatabaseEmpty` to require zero rows. The corresponding `On` methods accept an optional connection name.

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

### Soft deletion and model assertions {#soft-deletion-and-model-assertions}

`assertSoftDeleted` requires a matching row with a non-null `deleted_at`. `assertNotSoftDeleted` requires a matching row with a null `deleted_at`. For a custom column or named connection, use `assertSoftDeletedOn` or `assertNotSoftDeletedOn`.

Pass an Eloquent model to `assertModelExists` and `assertModelMissing`. They check the primary key on the model’s connection and count soft-deleted rows as present. `assertSoftDeletedModel` and `assertNotSoftDeletedModel` also use the model’s configured soft-deletion column.

```rust
use openrust::{database::eloquent::Model, testing::TestApp};

async fn assert_archived(
    test: &mut TestApp,
    model: &impl Model,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.assertModelExists(model).await?;
    test.assertSoftDeletedModel(model).await
}
```

### Database query counts {#expects-database-query-count}

Call `expectsDatabaseQueryCount(expected, None)` before the code you want to measure. The count includes observed queries from every connection in the test application. Pass `Some("reports")` to count queries from that named connection. Queries observed in `pretend` mode count, while failed SQL statements and queries with database events suppressed during fixture preparation do not.

Finish the test with `close().await` to check the number and wait for schema cleanup. A mismatch returns an error even though cleanup still runs. When a fixture goes out of scope normally, a mismatch fails the test.

```rust
use openrust::testing::TestApp;

async fn count_queries(
    mut test: TestApp,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.expectsDatabaseQueryCount(1, None).await?;
    test.database().await?.select("SELECT 1", &[]).await?;
    test.close().await
}
```
