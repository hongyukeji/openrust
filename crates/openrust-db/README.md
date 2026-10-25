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

The plugin installs a cloneable `Db` service and opens its pool during application finalization. `Db::connection()` returns a SeaORM `DatabaseConnection`; `Db::transaction()` commits successful callbacks and rolls back callbacks that return an error. Its `Debug` output does not include the configured URL. A missing URL, disabled backend, invalid pool settings, or failed connection stops startup with a configuration error.

For portable migration columns, `openrust::database::dialect::timestamp_utc(name, backend)` creates PostgreSQL `TIMESTAMP WITH TIME ZONE` or MySQL `DATETIME(6)`, and `dialect::json(name, backend)` creates PostgreSQL `JSONB` or MySQL `JSON`.

Register `MigrationPlugin::<Migrator>::new()` to add the runtime `migrate` command for an application migrator implementing `sea_orm_migration::MigratorTrait`. The migration API is re-exported as `openrust::database::sea_orm_migration`. Run `migrate` or `migrate up [steps]` to apply pending migrations, `migrate down [steps]` to roll back migrations (one by default), and `migrate status` to list applied and pending migrations. MySQL DDL may implicitly commit; if a migration fails, inspect and clean up its partial schema before retrying.

MySQL connections require TLS using Rustls. Configure the server certificate and trust chain through the MySQL URL/server deployment; plaintext MySQL connections are rejected. PostgreSQL TLS behavior follows the URL and server configuration.

## Scope

Connection pooling, transaction helpers, migration commands, and UTC/JSON dialect helpers are implemented. Seed support, per-test database isolation, and the `--db` project template remain in M2 scope.
