# Database

`DatabaseConfig` is defined in `openrust::config::database` and re-exported from `openrust::database`.
Runtime settings stay in the generated application's root `config/` directory; use `.env` for local
credentials and process environment variables for deployed secrets. Production bootstrap ignores
`.env`.

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

The plugin installs a cloneable `Db` service and opens its pool during application finalization. Daily development uses PostgreSQL 18, MySQL 8.4, and Redis 8. Database integration CI uses PostgreSQL 18 and MySQL 8.4; Redis integration joins CI with the M5 cache implementation. The current runtime version check accepts the provisional minimums PostgreSQL 14+ and MySQL 8.0+. Full framework compatibility at these minimum versions must still be verified before v1 release. Redis has a provisional minimum of 6.2+ and a daily development baseline of 8; its driver and compatibility validation are pending M5. Additional version checks are reserved for significant version gaps or concrete compatibility risks. `Db::connection()` returns a SeaORM `DatabaseConnection`; `Db::transaction()` commits successful callbacks and rolls back callbacks that return an error. Its `Debug` output does not include configured URLs. A missing URL, disabled backend, invalid pool settings, or failed connection stops startup with a configuration error.

Optional read replicas are configured explicitly and must use the same backend:

```toml
[database]
url = "postgres://app:secret@primary/app"
read_replicas = ["postgres://app:secret@replica-a/app", "postgres://app:secret@replica-b/app"]
```

`Db::connection()` and `Db::primary_connection()` always return the primary. `Db::read_connection()` round-robins configured replicas, or returns the primary when none are configured. Use it only for read-only operations that can tolerate replication lag; the framework does not inspect SQL, retry queries, or fall back after errors. `Db::with_primary(future)` temporarily makes this handle's `read_connection()` return the primary for the current Tokio task, which is useful for a write followed by a read in one work unit. Independently spawned tasks do not inherit that scope. Transactions, migrations, seeds, and framework critical paths stay on the primary. A configured replica connection or backend validation failure stops startup. Every replica has its own pool using the configured pool limits, so the maximum total connection count is `(replicas + 1) * max_connections`. This configures connections only; database replication, consistency, and replica read-only permissions remain deployment responsibilities.

`DatabasePlugin` also registers the shared `openrust::core::RateLimiter` using PostgreSQL or MySQL storage. Add `DatabaseRateLimitMigration` to the application's migrator before using it; schema changes remain explicit and are never run automatically at startup:

```rust,ignore
use openrust::database::DatabaseRateLimitMigration;

fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(DatabaseRateLimitMigration)]
}
```

Use `RateLimiter::check` with a stable key containing the policy identity, a positive limit, and a whole-second window. The fixed-window counter is atomic across application nodes sharing the same database. It consumes over-limit attempts and returns HTTP 429 with `Retry-After`; storage or missing-table errors fail closed. Database rows store a SHA-256 digest rather than the raw key, which is pseudonymization and does not protect guessable values such as email addresses. Each store instance opportunistically cleans up to 512 expired rows every 256 consumes; custom stores must document their own retention behavior. Memory or local-file implementations are not supplied as shared cluster stores.

The model generator keeps integer auto-increment IDs by default. Opt into `UUIDv7` primary keys or soft deletion on both the entity and migration commands:

```bash
openrust generate model post --uuid-id --soft-delete title:string
openrust generate migration posts --uuid-id --soft-delete title:string
```

`--uuid-id` adds the `db-uuid` feature to both generated packages, stores UUID IDs in the selected database, and fills missing insert IDs with `UUIDv7` in the application process. `--soft-delete` adds nullable UTC `deleted_at` and an explicit `Entity::not_deleted()` query helper; queries and raw SQL that do not call that helper continue to include deleted rows. The model and migration options must match. UUID and soft deletion are opt-in and do not change existing projects or tables.

`apps/server/benches/db_routing.rs` 提供发布模式下主库与只读副本路由的 `SELECT 1` 微基准。运行 `cargo bench -p server --features db-routing-bench,db-postgres --bench db_routing`，或改用 `db-routing-bench,db-mysql`；设置 `OPENRUST_TEST_DATABASE_URL` 与 `OPENRUST_TEST_READ_REPLICA_URL`。结果只反映该窄查询的串行吞吐量及 p50/p95/p99，不代表业务负载，也不能证明副本提升了吞吐量。显式 `db-routing-bench` feature 避免默认 workspace 检查在没有后端时编译数据库基准。

Applications may replace `DatabasePlugin` with a custom plugin when they need a different SeaORM connection strategy. Register one `Db::default()` during `build`, then initialize it after connecting during `finish`. A custom plugin with replicas can call `initialize_with_replicas(primary, replicas)`; all connections must report the same SeaORM backend:

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

`DatabaseBackend` centralizes the official PostgreSQL/MySQL URL scheme and feature checks. `Db::initialize` accepts a SeaORM `DatabaseConnection`; `Db::backend()` reports the connected SeaORM backend. A custom plugin owns its connection configuration and must validate capabilities required by its application. The official v1 database engines are PostgreSQL and MySQL. Their provisional minimums are PostgreSQL 14+ and MySQL 8.0+; daily development and database CI use PostgreSQL 18 and MySQL 8.4 LTS. Extra version checks are added when major-version gaps or concrete compatibility risks justify them; full compatibility at the provisional minimums remains a v1 release requirement. Other SeaORM backends do not automatically gain framework-wide support: migrations, UTC/JSON columns, locking, feature selection, project generation, and `TestDatabase` isolation need backend-specific implementation and verification. MongoDB and other different data models are outside this contract. Switching connections does not migrate existing data.

For portable migration columns, `openrust::database::dialect::timestamp_utc(name, backend)` creates PostgreSQL `TIMESTAMP WITH TIME ZONE` or MySQL `DATETIME(6)`, and `dialect::json(name, backend)` creates PostgreSQL `JSONB` or MySQL `JSON`. Both return `Result<ColumnDef, DbErr>` and reject unhandled backends instead of silently using PostgreSQL SQL.

Register `MigrationPlugin::<Migrator>::new()` to add the runtime `migrate` command for an application migrator implementing `sea_orm_migration::MigratorTrait`. The migration API is re-exported as `openrust::database::sea_orm_migration`. Run `migrate` or `migrate up [steps]` to apply pending migrations, `migrate down [steps]` to roll back migrations (one by default), and `migrate status` to list applied and pending migrations. MySQL DDL may implicitly commit; if a migration fails, inspect and clean up its partial schema before retrying.

Register `SeedPlugin::new(seed_fn)` alongside `DatabasePlugin` to expose `db seed`. The async function receives a cloneable `Db` handle and returns `openrust::Result<()>`; the application owns seed data and idempotency.

MySQL connections require TLS using Rustls. Configure the server certificate and trust chain through the MySQL URL/server deployment; plaintext MySQL connections are rejected. PostgreSQL TLS behavior follows the URL and server configuration.

## Scope

Connection pooling, transaction helpers, custom SeaORM connection registration, migration commands, UTC/JSON dialect helpers, application-defined seed commands, per-test database isolation, and the PostgreSQL/MySQL project templates are implemented.
