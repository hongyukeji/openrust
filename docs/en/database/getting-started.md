# Database

## Configuration

The application reads `config/database.rs`. The default connection is `sqlite`. Create `database/database.sqlite` before opening it, or set `DB_DATABASE=:memory:` for a temporary database. Relative SQLite paths resolve from the application root. An in-memory connection uses one physical connection and a private cache unless its URL explicitly requests a shared cache.

For MySQL, set `DB_CONNECTION=mysql` and configure `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD`. A connection's `url`, populated by `DB_URL` in the application configuration, can supply the driver and credentials. MySQL requires TLS. Set `MYSQL_ATTR_SSL_CA` for certificate and host verification, or configure `ssl_mode` and `ssl_ca` on the connection.

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
