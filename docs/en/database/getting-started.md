# Database

## Configuration

When `DB_DATABASE` is omitted, MySQL, MariaDB, PostgreSQL and SQL Server derive the database name from the application name. The configuration’s `database_name` helper reuses ASCII transliteration, lowercases letters and replaces runs of spaces or special characters with an underscore. Leading digits and underscores are removed, so `123 Store` becomes `store`; an empty result returns a configuration error asking you to set `DB_DATABASE`. An explicit `DB_DATABASE` or non-empty `DB_URL` takes precedence over deriving a default database name. The result is limited to 63 ASCII bytes and has no trailing underscore. For example, `ExampleStore` becomes `examplestore`, and `My Shop-V2` becomes `my_shop_v2`. Explicit `DB_DATABASE` values are used verbatim. SQLite keeps its file-path default. Provision the chosen database before connecting, or set `DB_DATABASE` to the name of an existing database.

The application reads `config/database.rs`. The default connection is `sqlite`. Create `database/database.sqlite` before opening it, or set `DB_DATABASE=:memory:` for a temporary database. Relative SQLite paths resolve from the application root. An in-memory connection uses one physical connection and a private cache unless its URL explicitly requests a shared cache.

For MySQL, set `DB_CONNECTION=mysql` and configure `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD`. A connection's `url`, populated by `DB_URL` in the application configuration, can supply the driver and credentials. MySQL requires TLS. Set `MYSQL_ATTR_SSL_CA` for certificate and host verification, or configure `ssl_mode` and `ssl_ca` on the connection. `verify_identity` checks the certificate chain and server name. `verify_ca` checks the certificate chain.

### MariaDB {#mariadb}

Set `DB_CONNECTION=mariadb` and use the same host, database, credential and TLS settings as MySQL. Use `mariadb://` when supplying `DB_URL`. To verify the server certificate and name, set `MYSQL_ATTR_SSL_CA` to the server CA certificate and use a `DB_HOST` covered by the server certificate.

The MariaDB grammar supports JSON column updates and generated JSON values. `uuid()` uses `char(36)` before MariaDB 10.7 and the native `uuid` type from 10.7. The database queue uses `SKIP LOCKED` from MariaDB 10.6. See the [release matrix](../prologue/releases.md) for tested versions.

### SQL Server {#sql-server}

Set `DB_CONNECTION=sqlsrv` with `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD`. A connection URL uses `sqlsrv://`. OpenRust uses a Rust TDS client with TLS and server certificate verification. Set `DB_SSL_CA` to the issuer certificate when using a private CA, and use a host name or address covered by the server certificate.

Create the database on SQL Server 2017 or 2022 before connecting. Run `artisan migrate --force` before starting HTTP and the database queue worker. Check the [release matrix](../prologue/releases.md) for the tested SQL Server builds.

### MySQL 5.7 TLS {#mysql-57-tls}

MySQL 5.7.44 needs a server build that can negotiate TLS 1.2 with ECDHE. For the Oracle generic Linux archive, use an RSA server certificate and the following `mysqld` settings. Supply the CA, server certificate and private key through `ssl-ca`, `ssl-cert` and `ssl-key`.

```ini
[mysqld]
require_secure_transport=ON
tls_version=TLSv1.2
ssl_cipher=ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384
```

Set the application's `MYSQL_ATTR_SSL_CA` to the CA certificate and use a `DB_HOST` present in the server certificate.

#### Oracle Linux 7 image {#mysql-57-oracle-linux-7}

The original `mysql:5.7` image lacks the server-side ECDHE setup needed by OpenRust. Build the derived image from the repository root. It enables ephemeral curve selection for the pinned MySQL 5.7.44 server and applies the TLS settings above.

```sh
docker build --platform linux/amd64 -t openrust/mysql57-ecdhe docker/mysql57-ecdhe
```

Use `openrust/mysql57-ecdhe` with the original image's database mount and environment variables. Mount the RSA server certificate and key and pass `--ssl-ca`, `--ssl-cert` and `--ssl-key` with their container paths. The application verifies the certificate and server name using `MYSQL_ATTR_SSL_CA` and `DB_HOST`. See the [release matrix](../prologue/releases.md).

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

Selects normally use the reader. Writes and transactions use the writer. With `sticky: true`, subsequent selects in the same HTTP request or Artisan execution use the writer after a modifying statement. With a standalone `DatabaseManager`, routing state is shared by clones of the logical connection until you clear it with `forget_record_modification_state`. Use `select_from_write_connection` to run a single select on the writer.


## Running SQL Queries

Inside an application lifecycle, use `Db::select`, `Db::select_one`, `Db::scalar`, `Db::insert`, `Db::update`, `Db::delete` and `Db::statement`. Use `?` placeholders and pass values separately. `select_one` returns an optional row, and `scalar` returns SQL null when no row matches. A scalar query must return one column. `update` and `delete` return the affected row count. `unprepared` executes raw SQL and should only receive application-controlled text.

A named connection from `Db::connection(Some("name"))` shares an application-local pool. Queries are asynchronous and take typed values. MySQL unsigned integers and decimals preserve their full precision. Decimal results use text.

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

Use `Db::select_result_sets` for a prepared MySQL statement that returns several result sets, such as a stored procedure call. The results preserve their order and include empty sets. MySQL also returns an empty final status set for `CALL`.

Pass `true` to use normal read routing or `false` to use the write connection. Inside a transaction, call the method on the transaction to keep the query on its connection. SQLite returns `DatabaseError::UnsupportedOperation` for this API.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

async fn find_active_users(db: &Connection, active: bool) -> DatabaseResult<Vec<Vec<Row>>> {
    db.select_result_sets("CALL find_active_users(?)", &[active.into()], true).await
}
```

### Using Named Bindings {#using-named-bindings}

Use `:name` placeholders and pass the matching values with `Bindings::Named`. Keys can include the leading colon. The order of the keys is independent of the SQL.

Bind every named parameter, and use one placeholder style per statement. SQLite allows a name to appear more than once. With native MySQL prepares, use a different name for each occurrence.

```rust
use openrust::{database::{Bindings, DatabaseResult, Row}, support::facades::Db};

async fn find_user(id: i64) -> DatabaseResult<Option<Row>> {
    Db::select_one(
        "SELECT * FROM users WHERE id = :id",
        Bindings::Named(&[("id", id.into())]),
    ).await
}
```

### Using Multiple Database Connections

Use `Db::connection(Some("reporting"))` to select a configured connection by name. The `::read` and `::write` suffixes select the corresponding physical role, such as `Db::connection(Some("reporting::read"))`.

#### Direct Connection Alias {#direct-connection-alias}

On MySQL and SQLite, `Db::connection(Some("mysql::direct"))` (using your configured connection name) selects the write endpoint for both reads and writes. Query logs and events identify the role as `QueryRole::Direct`. Without a separate direct endpoint, the framework also uses the write endpoint.

For PostgreSQL, a nonempty `direct` configuration selects a separate endpoint for `Db::connection(Some("pgsql::direct"))`. Set `DB_DIRECT_HOST`, `DB_DIRECT_PORT`, `DB_DIRECT_USERNAME`, `DB_DIRECT_PASSWORD`, and `DB_DIRECT_SSLMODE` as needed. A named connection’s `direct` options inherit its base configuration. Migrations select that endpoint by default, including ledger access. Explicit `::read`, `::write`, and `::direct` suffixes preserve the caller’s choice.

For PgBouncer in transaction mode, set `DB_POOLED=true` or configure a PostgreSQL `direct` endpoint. Pooled reads and writes emulate prepares by default, using PostgreSQL’s simple query protocol with encoded bindings. This works with `max_prepared_statements=0`. Extension and user-defined types use the server’s text result format in this mode. Direct connections use native prepares by default. Set `emulate_prepares` on a connection or its `read`, `write`, or `direct` options to override the default. If a pooled endpoint uses native prepares, enable PgBouncer’s named statement tracking. Keep the application and server `search_path` settings aligned and use the direct endpoint for migrations.

#### Reconnecting and Purging Connections {#reconnecting-database-connections}

`Db::disconnect` retires the current read and write pools. The next asynchronous operation opens fresh pools. `Db::reconnect` refreshes them immediately from the current configuration, preserving the logical connection and its query log. `Db::purge` also removes the cached connection. The next lookup creates a new logical connection.

An in-flight transaction remains on its original session until it ends. Connections that already hold a session can finish their work while new operations use the replacement pools.

```rust
use openrust::{database::DatabaseResult, support::facades::Db};

async fn refresh_database() -> DatabaseResult<()> {
    Db::disconnect(None)?;
    let connection = Db::reconnect(None).await?;
    assert_eq!(connection.scalar("SELECT 1", &[]).await?, 1.into());
    Db::purge(None)?;
    Ok(())
}
```

#### Recovering Lost Database Connections {#recovering-lost-connections}

When the driver reports a lost connection while executing a statement outside a transaction, the connection refreshes its pools and retries the statement once. A successful retry produces one `QueryExecuted` event. A connection loss during the outermost `BEGIN` can also retry once.

An active transaction reports a disconnect to its caller. A streaming cursor can retry until it yields its first row. A write may execute twice if the server applied it before the response was lost.

### Previewing Queries {#previewing-queries}

Use `Db::pretend` or a named connection’s `pretend` to collect SQL without sending it to the server. Reads return an empty result, writes report success or zero affected rows according to the method, and query hooks and events still run. The returned entries contain rendered SQL and the original bindings. The preview keeps its own log and leaves the ordinary query log available for live operations.

```rust
use openrust::{
    database::{DatabaseResult, QueryLogEntry},
    support::facades::Db,
};

async fn preview_settings_update() -> DatabaseResult<Vec<QueryLogEntry>> {
    Db::pretend(|connection| {
        Box::pin(async move {
            connection
                .update("UPDATE settings SET enabled = ? WHERE id = ?", &[1.into(), 42.into()])
                .await?;
            Ok(())
        })
    })
    .await
}
```

The preview applies to the connection in the current async scope. Independently spawned tasks start outside that scope. Use `pretending()` to inspect the active mode, and `without_pretending` to explicitly run live operations within a preview. Transaction lifecycle operations require a live scope.

### Callbacks Before Query Execution {#before-executing}

Register `Db::before_executing` on the default connection or call `before_executing` on a named connection. The hooks receive the original SQL and bindings. They run in registration order before the database operation. An error stops the operation and the remaining hooks. A cursor calls the hooks on its first poll. Driver-level replay of the same query skips them.

```rust
use openrust::{database::{Bindings, DatabaseResult, Value}, support::facades::Db};

async fn read_value() -> DatabaseResult<Value> {
    Db::before_executing(|sql, bindings, _, _| {
        Box::pin(async move {
            if sql == "SELECT ? AS n" {
                assert!(matches!(bindings, Bindings::Positional([Value::Integer(42)])));
            }
            Ok(())
        })
    })?;

    Db::scalar("SELECT ? AS n", &[42.into()]).await
}
```

The final parameter is the active transaction, when present. Use that handle for any work the hook needs to perform in the same transaction.

### Connection Established Event {#connection-established}

Register for `ConnectionEstablished` to initialize an application connection. Resolving a connection returns a lazy handle. Its first asynchronous operation awaits the event before using the database pool. Use `establish().await` to deliver it explicitly. Each named connection delivers the event once per pool generation. A listener can await SQL on the event's connection directly. If dispatch returns a listener error, the initiating operation receives that error. If the dispatch is canceled while a listener is running, the next operation retries it.

```rust
use std::{error::Error, sync::Arc};
use openrust::{
    contracts::events::Event as EventPayload,
    database::events::ConnectionEstablished,
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::{Db, Event},
};

fn connected<'a>(
    payload: &'a dyn EventPayload,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        let event = payload.downcast_ref::<ConnectionEstablished>()
            .ok_or(EventError::InvalidPayload)?;
        println!("Established {}", event.connection.get_name());
        Ok(None)
    })
}

async fn subscribe() -> Result<(), Box<dyn Error + Send + Sync>> {
    Event::listen(ConnectionEstablished::NAME, Arc::new(connected), Delivery::Sync)?;
    Db::connection(None)?.establish().await?;
    Ok(())
}
```

### Listening for Query Events {#listening-for-query-events}

Use `Db::listen` to observe successful queries on the application's connections. `QueryExecuted` carries the original SQL and bindings, the read or write role, the connection and the elapsed `Duration`. A cursor emits after successful exhaustion and excludes time spent by the caller between rows.

### Monitoring Cumulative Query Time {#monitoring-cumulative-query-time}

Use `Db::when_querying_for_longer_than` to monitor the default connection, or register on a named connection. Each handler runs once after the connection's cumulative query time exceeds the threshold. `reset_total_query_duration` clears the elapsed time. `allow_query_duration_handlers_to_run_again` rearms the handlers.

```rust
use std::{sync::Arc, time::Duration};
use openrust::{
    contracts::events::Event as EventPayload,
    database::{DatabaseResult, events::QueryExecuted},
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::Db,
};

fn report_query<'a>(
    payload: &'a dyn EventPayload,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        let query = payload.downcast_ref::<QueryExecuted>()
            .ok_or(EventError::InvalidPayload)?;
        println!("Query on {} took {:?}", query.connection.get_name(), query.time);
        Ok(None)
    })
}

fn subscribe() -> DatabaseResult<()> {
    Db::listen(Arc::new(report_query))
}

fn monitor_cumulative_time() -> DatabaseResult<()> {
    Db::when_querying_for_longer_than(Duration::from_millis(500), Arc::new(report_query))
}
```

### Inspecting Query Logs {#inspecting-query-logs}

`QueryExecuted::to_raw_sql()` renders the original statement with its bindings for diagnostics. The renderer preserves placeholders inside quoted text, identifiers and comments. The database still executes prepared statements with separate bindings.

Query logging is disabled by default. Enable it for a connection or through `Db` for the default connection. Use `get_query_log` for SQL and bindings, and `get_raw_query_log` for SQL with values embedded. Use `disable_query_log` to stop recording and `flush_query_log` to clear stored entries.

```rust
use openrust::{database::{DatabaseResult, RawQueryLogEntry}, support::facades::Db};

async fn inspect_value() -> DatabaseResult<Vec<RawQueryLogEntry>> {
    Db::enable_query_log()?;
    let result = Db::select("SELECT ? AS inspected", &[42.into()]).await;
    Db::disable_query_log()?;
    result?;
    let log = Db::get_raw_query_log()?;
    Db::flush_query_log()?;
    Ok(log)
}
```

## Database Transactions

Use `Db::transaction` for the default connection or call `transaction` on a named connection. Execute SQL through the transaction handle passed to the callback. The handle holds one write connection through the final commit or rollback. The boxed future lets Rust borrow that handle across asynchronous queries. The second argument specifies total attempts. Use `1` for a single attempt. Concurrency failures can rerun the whole callback, so keep side effects outside it or make them safe to repeat.

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

For manual control, call `begin_transaction().await?`, then call `commit` or `roll_back(None)` on the returned handle. Calling `begin_transaction` on the handle creates a savepoint. `roll_back(Some(level))` rolls back to the given nesting level. Level zero aborts the whole transaction. Dropping an active handle rolls back its pending work. MySQL statements such as DDL can implicitly commit, so keep them outside transactions that require atomicity.

### Shared connection interface {#connection-interface}

Use `&mut dyn ConnectionInterface` when the same SQL or builder code runs on a pooled `Connection` or a pinned `Transaction`. Pass named or positional `Bindings` explicitly. For `select`, `select_one`, `scalar`, and `cursor`, passing `false` requests the writer on a pooled connection. A transaction always runs the query on its own session. Create, commit, and roll back transactions through the concrete connection and transaction handles.

```rust
use openrust::database::{Bindings, ConnectionInterface, DatabaseResult, Value};

pub async fn answer(connection: &mut dyn ConnectionInterface) -> DatabaseResult<Value> {
    connection
        .scalar("SELECT :answer AS answer", Bindings::Named(&[("answer", 42.into())]), false)
        .await
}
```

### SQL Server Automatic Transaction Rollback {#sql-server-automatic-rollback}

SQL Server can roll back an entire transaction when a statement fails. When a transaction callback returns that error, OpenRust checks the server’s transaction state before it issues a rollback. If the server already rolled back, the callback returns the original statement error and the transaction handle closes. If the remaining transaction cannot roll back to a savepoint, OpenRust rolls back the whole transaction. On a confirmed rollback, applicable `after_roll_back()` callbacks run.

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
                println!("Committed on {}", event.connection.get_name());
            }
            Ok(None)
        })
    }
}

fn register() -> EventResult<()> {
    Event::listen(TransactionCommitted::NAME, Arc::new(ReportCommit), Delivery::Sync)
}
```

### Transaction Callbacks {#transaction-callbacks}

`Db::before_starting_transaction` registers a hook on the default connection. It runs before an outer transaction or savepoint starts. The hook receives the connection and, for a nested start, the active transaction. Returning an error stops that start.

`Transaction::after_roll_back` registers a callback for the current level. Callbacks from a committed savepoint remain attached to its parent until the outer transaction finishes. A confirmed rollback runs callbacks for the discarded levels. Outer rollback callbacks run after the session has been released.

```rust
use openrust::{database::DatabaseResult, support::facades::Db};

async fn run_with_callbacks() -> DatabaseResult<()> {
    Db::before_starting_transaction(|connection, active| {
        Box::pin(async move {
            match active {
                Some(transaction) => transaction.scalar("SELECT 1", &[]).await?,
                None => connection.scalar("SELECT 1", &[]).await?,
            };
            Ok(())
        })
    })?;

    let mut transaction = Db::begin_transaction().await?;
    transaction.after_roll_back(|| Box::pin(async {
        println!("Transaction rolled back");
        Ok(())
    }))?;
    transaction.roll_back(None).await
}
```
