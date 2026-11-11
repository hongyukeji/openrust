# Database

## Configuration

The application reads `config/database.rs`. The default connection is `sqlite`. Create `database/database.sqlite` before opening it, or set `DB_DATABASE=:memory:` for a temporary database. Relative SQLite paths resolve from the application root. An in-memory connection uses one physical connection and a private cache unless its URL explicitly requests a shared cache.

For MySQL, set `DB_CONNECTION=mysql` and configure `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD`. A connection's `url`, populated by `DB_URL` in the application configuration, can supply the driver and credentials. MySQL requires TLS. Set `MYSQL_ATTR_SSL_CA` for certificate and host verification, or configure `ssl_mode` and `ssl_ca` on the connection.

### Read and Write Connections

Set `read` and `write` inside a named connection in `config/database.rs`. Each role inherits the common options and overrides its own values. For MySQL, a host list is shuffled when the pool is configured, and acquisition tries the next host when a host is unavailable. Supply the password from your environment.

```json
{
  "driver": "mysql",
  "database": "app",
  "username": "app",
  "read": { "host": ["replica-1", "replica-2"] },
  "write": { "host": "primary" },
  "sticky": true
}
```

Selects normally use the reader. Writes and transactions use the writer. With `sticky: true`, subsequent selects in the same HTTP request or Artisan execution use the writer after a modifying statement. With a standalone `DatabaseManager`, routing state is shared by clones of the logical connection until you clear it with `forgetRecordModificationState`. Use `selectFromWriteConnection` to run a single select on the writer.


## Running SQL Queries

Inside an application lifecycle, use `DB::select`, `DB::selectOne`, `DB::scalar`, `DB::insert`, `DB::update`, `DB::delete` and `DB::statement`. Use `?` placeholders and pass values separately. `selectOne` returns an optional row, and `scalar` returns SQL null when no row matches. A scalar query must return one column. `update` and `delete` return the affected row count. `unprepared` executes raw SQL and should only receive application-controlled text.

A named connection from `DB::connection(Some("name"))` shares an application-local pool. Queries are asynchronous and take typed values. MySQL unsigned integers and decimals preserve their full precision. Decimal results use text.

```rust
use openrust::{config::Repository, database::{DatabaseManager, Value}};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = DatabaseManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let connection = manager.connection(None)?;
    connection.statement("CREATE TABLE IF NOT EXISTS notes (body TEXT)", &[]).await?;
    connection.insert("INSERT INTO notes VALUES (?)", &["A bound value".into()]).await?;
    let value = connection.scalar("SELECT body FROM notes WHERE body = ?", &["A bound value".into()]).await?;
    assert_eq!(value, Value::from("A bound value"));
    Ok(())
}
```

### Using Multiple Database Connections

Use `DB::connection(Some("reporting"))` to select a configured connection by name. The `::read` and `::write` suffixes select the corresponding physical role, such as `DB::connection(Some("reporting::read"))`.

## Database Transactions

Use `DB::transaction` for the default connection or call `transaction` on a named connection. Execute SQL through the transaction handle passed to the callback. The handle holds one write connection through the final commit or rollback. The boxed future lets Rust borrow that handle across asynchronous queries. The second argument specifies total attempts. Use `1` for a single attempt. Concurrency failures can rerun the whole callback, so keep side effects outside it or make them safe to repeat.

```rust
use openrust::database::{Connection, DatabaseResult, Value};

pub async fn add_note(connection: &Connection) -> DatabaseResult<Value> {
    connection
        .transaction(
            |transaction| {
                Box::pin(async {
                    transaction
                        .insert("INSERT INTO notes (body) VALUES (?)", &["hello".into()])
                        .await?;
                    transaction
                        .scalar("SELECT COUNT(*) FROM notes WHERE body = ?", &["hello".into()])
                        .await
                })
            },
            3,
        )
        .await
}
```

For manual control, call `beginTransaction().await?`, then call `commit` or `rollBack(None)` on the returned handle. Calling `beginTransaction` on the handle creates a savepoint. `rollBack(Some(level))` rolls back to the given nesting level. Level zero aborts the whole transaction. Dropping an active handle rolls back its pending work. MySQL statements such as DDL can implicitly commit, so keep them outside transactions that require atomicity.
