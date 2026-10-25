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

The plugin installs a cloneable `Db` service and opens its pool during application finalization. It checks the connected server is PostgreSQL 14+ or MySQL 8.0+; an older or unrecognized version stops startup. `Db::connection()` returns a SeaORM `DatabaseConnection`; `Db::transaction()` commits successful callbacks and rolls back callbacks that return an error. Its `Debug` output does not include the configured URL. A missing URL, disabled backend, invalid pool settings, or failed connection stops startup with a configuration error.

Applications may replace `DatabasePlugin` with a custom plugin when they need a different SeaORM connection strategy. Register one `Db::default()` during `build`, then initialize it after connecting during `finish`:

```rust,ignore
impl Plugin for CustomDatabasePlugin {
    fn build(&self, app: &mut AppBuilder) -> openrust::Result<()> {
        if app.get::<Db>().is_some() {
            return Err(openrust::Error::custom("duplicate_database", "only one database plugin is allowed"));
        }
        app.insert(Db::default());
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let connection = connect_with_custom_policy().await?;
            app.state().try_get::<Db>()?.initialize(connection)
        })
    }
}
```

`DatabaseBackend` centralizes the official PostgreSQL/MySQL URL scheme and feature checks. `Db::initialize` accepts a SeaORM `DatabaseConnection`; `Db::backend()` reports the connected SeaORM backend. A custom plugin owns its connection configuration and must validate capabilities required by its application. The official v1 matrix remains PostgreSQL 14+ and MySQL 8.0+. Other SeaORM backends do not automatically gain framework-wide support: migrations, UTC/JSON columns, locking, feature selection, project generation, and `TestDatabase` isolation need backend-specific implementation and verification. MongoDB and other different data models are outside this contract. Switching connections does not migrate existing data.

For portable migration columns, `openrust::database::dialect::timestamp_utc(name, backend)` creates PostgreSQL `TIMESTAMP WITH TIME ZONE` or MySQL `DATETIME(6)`, and `dialect::json(name, backend)` creates PostgreSQL `JSONB` or MySQL `JSON`. Both return `Result<ColumnDef, DbErr>` and reject unhandled backends instead of silently using PostgreSQL SQL.

Register `MigrationPlugin::<Migrator>::new()` to add the runtime `migrate` command for an application migrator implementing `sea_orm_migration::MigratorTrait`. The migration API is re-exported as `openrust::database::sea_orm_migration`. Run `migrate` or `migrate up [steps]` to apply pending migrations, `migrate down [steps]` to roll back migrations (one by default), and `migrate status` to list applied and pending migrations. MySQL DDL may implicitly commit; if a migration fails, inspect and clean up its partial schema before retrying.

Register `SeedPlugin::new(seed_fn)` alongside `DatabasePlugin` to expose `db seed`. The async function receives a cloneable `Db` handle and returns `openrust::Result<()>`; the application owns seed data and idempotency.

MySQL connections require TLS using Rustls. Configure the server certificate and trust chain through the MySQL URL/server deployment; plaintext MySQL connections are rejected. PostgreSQL TLS behavior follows the URL and server configuration.

## Scope

Connection pooling, transaction helpers, custom SeaORM connection registration, migration commands, UTC/JSON dialect helpers, application-defined seed commands, per-test database isolation, and the PostgreSQL/MySQL project templates are implemented.
