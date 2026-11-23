# Database Testing

## Isolating the database

Use `TestApp::sqlite` to give each test application its own SQLite file. During provider registration the fixture replaces the configured connections and selects SQLite. The file lives under the test root, so tests can run in parallel with independent data.

## Migrations and seeders

Register application migrations and seeders in the builder callback as you would in the application. Then run `test.artisan(["migrate", "--seed"]).await.assert_successful()`. To rebuild the schema and seed again in the same fixture, use `migrate:fresh --seed`. Each new fixture starts with an empty database file. Each database file provides a separate test boundary.

### Running seeders {#running-seeders}

Call `test.seed().await?` to run the registered `DatabaseSeeder`. For one or more named seeders, pass their names to `seed_with`. The list runs in order and an empty list is a no-op. Use `seed_on("reports", None)` for the default seeder on a named connection, or pass `Some(&[...])` to select specific seeders.

Each seeder runs through Artisan `db:seed` with prompts disabled. A failed command returns an error and stops the list. Inside a test transaction, the writes on each selected connection roll back with that transaction. Other connections retain their own commit behavior.

```rust
use openrust::testing::TestApp;

async fn populate(
    test: &mut TestApp,
    seeders: &[&str],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.seed().await?;
    test.seed_with(seeders).await
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
    test.artisan(["migrate"]).await.assert_successful();
    test.close().await
}
```

Call `close().await` to wait for cleanup. It closes application pools and deletes the schema only after verifying the management lease, ownership marker, and absence of other connected sessions. If a check fails, the error reports the schema name for inspection. A dropped fixture attempts cleanup while its Tokio runtime is running.

### Recovering an abandoned MySQL test schema {#recovering-mysql-test-schemas}

To recover after a test process is terminated, use `TestApp::mysql_with_recovery` or `TestApp::mysql_connections_with_recovery`. Pass an existing private directory on the local filesystem, outside the fixture’s temporary root. On Unix, use mode `0700`. Each schema gets an `openrust_test_<id>.json` receipt before creation. The receipt contains a server identifier and an ownership token. Keep it private. Connection URLs and credentials stay outside the receipt.

From a fresh process, pass the receipt path and the original dedicated test server’s management URL. Recovery checks the server UUID, acquires the schema lease, and verifies the ownership routine and server activity before dropping a schema. When a lease is held, the schema is busy, or ownership cannot be verified, recovery returns an error and keeps the receipt. Successful cleanup removes it. Repeating recovery after cleanup succeeds without taking any action.

```rust
use std::path::Path;
use openrust::testing::TestApp;

async fn recover_test_schema(admin_url: &str, receipt: &Path) -> std::io::Result<()> {
    TestApp::recover_mysql(admin_url, receipt).await
}
```

## Refreshing the database {#refreshing-the-database}

Use `refresh_database` with a managed MySQL fixture. It runs `migrate:fresh` before the first callback and reuses the migrated baseline for later callbacks with the same options. Each callback runs in its own rollback transaction. A named seeder takes precedence over `seed`.

```rust
use openrust::testing::{RefreshDatabaseOptions, TestApp};

async fn check_baseline(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = RefreshDatabaseOptions { seed: true, ..Default::default() };
    test.refresh_database(options, |test| Box::pin(async move {
        test.assert_database_count("items", 1).await
    })).await
}
```

A migration or Seeder failure returns before the callback and leaves the fixture ready to retry preparation. A callback error with a successful rollback preserves the baseline. An interrupted transaction or changed options rebuilds it on the next call. If a caller is cancelled during migration, the next refresh waits for that migration to finish first.

Set `without_migration_events: true` to mute query and other connection events on the selected connections during preparation. Migration lifecycle events, query logs, and events from other connections remain available. The test callback and commands run outside preparation dispatch connection events normally.

## Rebuilding and rolling back migrations {#database-migrations}

Use `run_database_migrations` when a test needs to commit its own transactions. On the fixture’s managed default MySQL connection, it runs `migrate:fresh` before the callback and `migrate:rollback` afterward, including when the callback returns an error or panics. The rollback targets the same schema used for preparation.

Set `drop_views` to include views in the rebuild. Set `seed` to run the default seeder, or supply `seeder` to select a named one. After cancelling the future, call `close().await` or start another database reset to finish the pending rollback.

```rust
use openrust::testing::{DatabaseMigrationsOptions, TestApp};

async fn check_empty_items(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.run_database_migrations(DatabaseMigrationsOptions::default(), |test| {
        Box::pin(async { test.assert_database_empty("items").await })
    }).await
}
```

## Truncating tables between tests {#database-truncation}

Call `truncate_database_tables` before each test on the managed fixture. The first call runs `migrate:fresh` on the default connection. Later calls truncate populated tables on the selected connections and reseed the default connection when requested. Set `drop_views` for the first rebuild, and use `seed` or `seeder` to select how data is restored.

An empty `connections_to_truncate` selects the default connection. Use `tables` for shared filters and `tables_by_connection` for connection-specific filters. Match physical catalog names, either bare or schema-qualified. A nonempty `only` list selects the tables to truncate. Otherwise, `except` and the migration ledger are retained.

The reset uses a dedicated write session to disable and restore foreign-key checks. MySQL truncation commits implicitly, so call this method outside test transactions. If the future is cancelled, the next reset or `close().await` waits for the pending work.

```rust
use openrust::testing::{DatabaseTruncationOptions, TestApp};

async fn reset_items(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.truncate_database_tables(DatabaseTruncationOptions {
        seed: true,
        ..Default::default()
    }).await?;
    test.assert_database_count("items", 1).await
}
```

## MySQL transaction isolation {#mysql-transaction-isolation}

Configure the test application to use a dedicated MySQL test database. Run migrations and baseline seeders before calling `test.database_transaction`. Inside the callback, HTTP requests, in-process Artisan commands, and database assertions on the default connection share one session. The helper rolls back on success or error, and application transactions use savepoints.

```rust
use openrust::testing::TestApp;

async fn check_seeded_items(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.database_transaction(|test| Box::pin(async move {
        test.artisan(["db:seed"]).await.assert_successful();
        test.assert_database_has("items", &[("name", "seeded".into())]).await
    })).await
}
```

Use InnoDB tables. Run schema changes outside the callback. Await each operation on the session before starting another. Spawned tasks, external processes, and other named connections run outside the callback’s transaction.

## Multiple test connections {#multiple-test-connections}

Call `TestApp::mysql_connections` with named server URLs to create an owned schema for each connection. The first name becomes the default. With `refresh_database`, `migrate:fresh` prepares the default connection. Prepare any secondary schemas with `migrate --database=<name>` before the callback.

```rust
use openrust::testing::{RefreshDatabaseOptions, TestApp};

async fn run_test(
    url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::mysql_connections(&[("mysql", url), ("reports", url)], |builder| {
        builder
    }).await?;
    test.artisan(["migrate", "--database=reports"]).await.assert_successful();
    let options = RefreshDatabaseOptions {
        connections_to_transact: vec!["mysql".into(), "reports".into()],
        ..Default::default()
    };
    test.refresh_database(options, |_| Box::pin(async { Ok(()) })).await?;
    test.close().await
}
```

Set `connections_to_transact` to select the connections that roll back after each callback. An empty list selects the default. For an already prepared fixture, `database_transaction_on` runs the same rollback lifecycle without a schema refresh.

## Model factories {#model-factories}

Create test records with the model’s factory before issuing requests or checking database state. The factory uses the application’s Eloquent connection, so its writes follow the same fixture transaction rules. See [Eloquent factories](../eloquent/factories.md) for factory definitions and states.

## Database assertions

Use `assert_database_has` and `assert_database_missing` to check row values, including nulls. Use `assert_database_count` to check the row count and `assert_database_empty` to require zero rows. The corresponding `On` methods accept an optional connection name.

```rust
use openrust::{database::query::Record, testing::TestApp};

#[tokio::test]
async fn keeps_records_in_the_test_database() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::sqlite(|builder| builder)?;
    let db = test.database().await?;
    db.get_schema_builder()
        .create("items", |table| {
            table.id();
            table.string("name", 40);
        })
        .await?;
    db.table("items")
        .insert(&[Record::from([("name".into(), "fixture".into())])])
        .await?;
    test.assert_database_has("items", &[("name", "fixture".into())]).await?;
    test.assert_database_count("items", 1).await?;
    Ok(())
}
```

### Soft deletion and model assertions {#soft-deletion-and-model-assertions}

`assert_soft_deleted` requires a matching row with a non-null `deleted_at`. `assert_not_soft_deleted` requires a matching row with a null `deleted_at`. For a custom column or named connection, use `assert_soft_deleted_on` or `assert_not_soft_deleted_on`.

Pass an Eloquent model to `assert_model_exists` and `assert_model_missing`. They check the primary key on the model’s connection and count soft-deleted rows as present. `assert_soft_deleted_model` and `assert_not_soft_deleted_model` also use the model’s configured soft-deletion column.

```rust
use openrust::{database::eloquent::Model, testing::TestApp};

async fn assert_archived(
    test: &mut TestApp,
    model: &impl Model,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.assert_model_exists(model).await?;
    test.assert_soft_deleted_model(model).await
}
```

### Database query counts {#expects-database-query-count}

Call `expects_database_query_count(expected, None)` before the code you want to measure. The count includes observed queries from every connection in the test application. Pass `Some("reports")` to count queries from that named connection. Queries observed in `pretend` mode count, while failed SQL statements and queries with database events suppressed during fixture preparation do not.

Finish the test with `close().await` to check the number and wait for schema cleanup. A mismatch returns an error even though cleanup still runs. When a fixture goes out of scope normally, a mismatch fails the test.

```rust
use openrust::testing::TestApp;

async fn count_queries(
    mut test: TestApp,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.expects_database_query_count(1, None).await?;
    test.database().await?.select("SELECT 1", &[]).await?;
    test.close().await
}
```
