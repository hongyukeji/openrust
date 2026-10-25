# openrust-db

Database layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework (milestone M2).

## Connect a database

Enable one backend feature and register `DatabasePlugin` with the application:

```toml
[dependencies]
openrust = { version = "0.0.0", features = ["db-postgres"] }
```

Set `DATABASE_URL` or `OPENRUST_DATABASE__URL` to a PostgreSQL or MySQL connection URL. Pool defaults are 10 maximum connections, 1 minimum connection, a 5-second connect timeout, and a 30-second acquire timeout. Override them in `[database]` or with `OPENRUST_DATABASE__MAX_CONNECTIONS`, `MIN_CONNECTIONS`, `CONNECT_TIMEOUT_SECS`, and `ACQUIRE_TIMEOUT_SECS`.

```rust,ignore
use openrust::{Application, AppBuilder, DatabasePlugin, NoConfig};

struct App;

impl Application for App {
    type Config = NoConfig;

    fn build(builder: &mut AppBuilder) {
        builder.plugin(DatabasePlugin);
    }
}
```

The plugin installs a cloneable `Db` service and opens its pool during application finalization. `Db::connection()` returns a SeaORM `DatabaseConnection`; its `Debug` output does not include the configured URL. A missing URL, disabled backend, invalid pool settings, or failed connection stops startup with a configuration error.

MySQL connections require TLS using Rustls. Configure the server certificate and trust chain through the MySQL URL/server deployment; plaintext MySQL connections are rejected. PostgreSQL TLS behavior follows the URL and server configuration.

## Scope

The connection provider is implemented. Migration execution, transaction helpers, seed support, database commands, dialect helpers, per-test database isolation, and the `--db` project template remain in M2 scope.
