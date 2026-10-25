# openrust-test

Testing utilities for [OpenRust](https://github.com/hongyukeji/openrust) applications.

## Isolated database

Enable the backend used by the test database and pass an admin URL that can create and drop databases:

```toml
[dev-dependencies]
openrust-test = { version = "0.0.0", features = ["db-postgres"] }
```

```rust,ignore
let database = openrust_test::TestDatabase::create::<migration::Migrator>(
    &std::env::var("OPENRUST_TEST_DATABASE_URL")?,
)
.await?;
let test_url = database.url();
// Configure and run the application against test_url.
database.close().await?;
```

Each instance receives a unique database and runs the application's migrations. PostgreSQL
creates a temporary migrated template and clones it; MySQL creates a database and applies the
migrations. The URL's database path is replaced while its credentials and query parameters remain.
The admin user needs database creation and deletion privileges. MySQL connections use required
TLS and set session time zone to UTC. Explicit `close().await` waits for deletion; dropping the guard schedules best-effort cleanup
while a Tokio runtime is active.

## Scope

Per-test database isolation is implemented. `TestApp`, HTTP response assertions, factories, and
fake-driver assertions remain planned for their later milestones.
