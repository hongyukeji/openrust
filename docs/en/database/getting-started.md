# Database

## Configuration

The application reads `config/database.rs`. The default connection is `sqlite`. Create `database/database.sqlite` before opening it, or set `DB_DATABASE=:memory:` for a temporary database. Relative SQLite paths resolve from the application root. An in-memory connection uses one physical connection and a private cache unless its URL explicitly requests a shared cache.

For MySQL, set `DB_CONNECTION=mysql` and configure `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD`. A connection's `url`, populated by `DB_URL` in the application configuration, can supply the driver and credentials. MySQL requires TLS. Set `MYSQL_ATTR_SSL_CA` for certificate and host verification, or configure `ssl_mode` and `ssl_ca` on the connection. `verify_identity` checks the certificate chain and server name. `verify_ca` checks the certificate chain.

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

### Selecting Multiple Result Sets {#selecting-multiple-result-sets}

Use `DB::selectResultSets` for a prepared MySQL statement that returns several result sets, such as a stored procedure call. The results preserve their order and include empty sets. MySQL also returns an empty final status set for `CALL`.

Pass `true` to use normal read routing or `false` to use the write connection. Inside a transaction, call the method on the transaction to keep the query on its connection. SQLite returns `DatabaseError::UnsupportedOperation` for this API.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

async fn find_active_users(db: &Connection, active: bool) -> DatabaseResult<Vec<Vec<Row>>> {
    db.selectResultSets("CALL find_active_users(?)", &[active.into()], true).await
}
```

### Using Named Bindings {#using-named-bindings}

Use `:name` placeholders and pass the matching values with `Bindings::Named`. Keys can include the leading colon. The order of the keys is independent of the SQL.

Bind every named parameter, and use one placeholder style per statement. SQLite allows a name to appear more than once. With native MySQL prepares, use a different name for each occurrence.

```rust
use openrust::{database::{Bindings, DatabaseResult, Row}, support::facades::DB};

async fn find_user(id: i64) -> DatabaseResult<Option<Row>> {
    DB::selectOne(
        "SELECT * FROM users WHERE id = :id",
        Bindings::Named(&[("id", id.into())]),
    ).await
}
```

### Using Multiple Database Connections

Use `DB::connection(Some("reporting"))` to select a configured connection by name. The `::read` and `::write` suffixes select the corresponding physical role, such as `DB::connection(Some("reporting::read"))`.

### Connection Established Event {#connection-established}

Register for `ConnectionEstablished` to initialize an application connection. Resolving a connection returns a lazy handle. Its first asynchronous operation awaits the event before using the database pool. Use `establish().await` to deliver it explicitly. Each named connection delivers the event once per application. A listener can await SQL on the event's connection directly. If dispatch returns a listener error, the initiating operation receives that error. If the dispatch is canceled while a listener is running, the next operation retries it.

```rust
use std::{error::Error, sync::Arc};
use openrust::{
    contracts::events::Event as EventPayload,
    database::events::ConnectionEstablished,
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::{DB, Event},
};

fn connected<'a>(
    payload: &'a dyn EventPayload,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        let event = payload.downcast_ref::<ConnectionEstablished>()
            .ok_or(EventError::InvalidPayload)?;
        println!("Established {}", event.connection.getName());
        Ok(None)
    })
}

async fn subscribe() -> Result<(), Box<dyn Error + Send + Sync>> {
    Event::listen(ConnectionEstablished::NAME, Arc::new(connected), Delivery::Sync)?;
    DB::connection(None)?.establish().await?;
    Ok(())
}
```

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

### Transaction Events {#transaction-events}

Subscribe through the application event dispatcher. `TransactionBeginning` fires after a transaction or savepoint starts. `TransactionCommitting` fires immediately before the outer commit. `TransactionCommitted` fires after the transaction level decreases and the applicable commit callbacks have run. `TransactionRolledBack` fires once after an explicit rollback or the rollback of a failed transaction callback.

A listener can use the transaction in its event context while a level is active. After an outer commit or rollback, the event carries the connection and level zero. A listener failure after the outer commit returns `DatabaseError::AfterCommit` and the committed data remains.

```rust
use std::sync::Arc;
use openrust::{
    contracts::events::{Event as EventPayload, Listener},
    database::events::TransactionCommitted,
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::Event,
};

struct ReportCommit;

impl Listener for ReportCommit {
    fn handle<'a>(
        &'a self,
        payload: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = payload.downcast_ref::<TransactionCommitted>()
                .ok_or(EventError::InvalidPayload)?;
            if event.level == 0 {
                println!("Committed on {}", event.connection.getName());
            }
            Ok(None)
        })
    }
}

fn register() -> EventResult<()> {
    Event::listen(TransactionCommitted::NAME, Arc::new(ReportCommit), Delivery::Sync)
}
```
