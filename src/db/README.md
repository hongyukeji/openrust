# Database

Database module of the [OpenRust](https://github.com/hongyukeji/openrust) framework (milestone M2).

## Connect a database

Enable one backend feature and register `DatabasePlugin` with the application:

```toml
[dependencies]
openrust = { version = "0.0.0", features = ["db-postgres"] }
```

Set `DATABASE_URL` or `APP_DATABASE__URL` to a PostgreSQL or MySQL connection URL. Pool defaults are 10 maximum connections, 1 minimum connection, a 5-second connect timeout, and a 30-second acquire timeout. Override them in `[database]` or with `APP_DATABASE__MAX_CONNECTIONS`, `MIN_CONNECTIONS`, `CONNECT_TIMEOUT_SECS`, and `ACQUIRE_TIMEOUT_SECS`. Applications using a custom namespace replace `APP_` with the value returned by `Application::config_namespace()`.

Framework-owned tables use the `tb_` prefix by default. Set `table_prefix` to change it (an empty string disables the prefix), or map selected logical names to complete physical table names:

```toml
[database]
table_prefix = "app_"

[database.table_names]
users = "accounts"
```

Explicit mappings take precedence and are not prefixed. Names must be ASCII identifiers; PostgreSQL's 63-byte and MySQL's 64-byte limits are checked at startup. `Db::table_name()` returns a validated physical name and `Db::quoted_table_name()` returns its backend-quoted form. `MigrationPlugin` and `TestDatabase::create_with_table_names()` scope the same resolver to each migration call. Handwritten migrations that create framework-related tables can read `DatabaseTableNames::current()`; calls outside those migration scopes return `None`. Raw SQL is not globally rewritten: framework authentication SQL resolves its own table names, while application SQL must use the `Db` resolver. Renaming configured tables does not migrate existing schemas or data.

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

The plugin installs a cloneable `Db` service and opens its pool during application finalization. Daily development and CI use PostgreSQL 16 and MySQL 8.4 LTS. The current runtime version check accepts PostgreSQL 16+ and MySQL 8.4 LTS+; this is not the final release support floor, which will be set and verified before v1 release. Additional version checks are reserved for significant version gaps or concrete compatibility risks. `Db::connection()` returns a SeaORM `DatabaseConnection`; `Db::transaction()` commits successful callbacks and rolls back callbacks that return an error. Its `Debug` output does not include the configured URL. A missing URL, disabled backend, invalid pool settings, or failed connection stops startup with a configuration error.

`DatabasePlugin` also registers the shared `openrust::core::RateLimiter` using PostgreSQL or MySQL storage. Add `DatabaseRateLimitMigration` to the application's migrator before using it; schema changes remain explicit and are never run automatically at startup:

```rust,ignore
use openrust::database::DatabaseRateLimitMigration;

fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(DatabaseRateLimitMigration)]
}
```

Use `RateLimiter::check` with a stable key containing the policy identity, a positive limit, and a whole-second window. The fixed-window counter is atomic across application nodes sharing the same database. It consumes over-limit attempts and returns HTTP 429 with `Retry-After`; storage or missing-table errors fail closed. Database rows store a SHA-256 digest rather than the raw key, which is pseudonymization and does not protect guessable values such as email addresses. Each store instance opportunistically cleans up to 512 expired rows every 256 consumes; custom stores must document their own retention behavior. Memory or local-file implementations are not supplied as shared cluster stores.

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

`DatabaseBackend` centralizes the official PostgreSQL/MySQL URL scheme and feature checks. `Db::initialize` accepts a SeaORM `DatabaseConnection`; `Db::backend()` reports the connected SeaORM backend. A custom plugin owns its connection configuration and must validate capabilities required by its application. The official v1 database engines are PostgreSQL and MySQL. Daily development and CI targets are PostgreSQL 16 and MySQL 8.4 LTS; the current PostgreSQL runtime version check still accepts 16+, and extra version checks are added only when major-version gaps or compatibility risks justify them. The minimum supported versions will be determined and verified before v1 release. Other SeaORM backends do not automatically gain framework-wide support: migrations, UTC/JSON columns, locking, feature selection, project generation, and `TestDatabase` isolation need backend-specific implementation and verification. MongoDB and other different data models are outside this contract. Switching connections does not migrate existing data.

For portable migration columns, `openrust::database::dialect::timestamp_utc(name, backend)` creates PostgreSQL `TIMESTAMP WITH TIME ZONE` or MySQL `DATETIME(6)`, and `dialect::json(name, backend)` creates PostgreSQL `JSONB` or MySQL `JSON`. Both return `Result<ColumnDef, DbErr>` and reject unhandled backends instead of silently using PostgreSQL SQL.

Register `MigrationPlugin::<Migrator>::new()` to add the runtime `migrate` command for an application migrator implementing `sea_orm_migration::MigratorTrait`. The migration API is re-exported as `openrust::database::sea_orm_migration`. Run `migrate` or `migrate up [steps]` to apply pending migrations, `migrate down [steps]` to roll back migrations (one by default), and `migrate status` to list applied and pending migrations. MySQL DDL may implicitly commit; if a migration fails, inspect and clean up its partial schema before retrying.

Register `SeedPlugin::new(seed_fn)` alongside `DatabasePlugin` to expose `db seed`. The async function receives a cloneable `Db` handle and returns `openrust::Result<()>`; the application owns seed data and idempotency.

MySQL connections require TLS using Rustls. Configure the server certificate and trust chain through the MySQL URL/server deployment; plaintext MySQL connections are rejected. PostgreSQL TLS behavior follows the URL and server configuration.

## Scope

Connection pooling, transaction helpers, custom SeaORM connection registration, migration commands, UTC/JSON dialect helpers, application-defined seed commands, per-test database isolation, and the PostgreSQL/MySQL project templates are implemented.
