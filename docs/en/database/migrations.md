# Migrations

## Generating Migrations

Use `make:migration` to create a timestamped Rust source file in `database/migrations`. Use `--create=flights` or `--table=flights` to select the stub explicitly.

```sh
./artisan make:migration create_flights_table
```

During a build, OpenRust discovers Rust migrations in `database/migrations/`. Use the `YYYY_MM_DD_HHMMSS_name.rs` filename format and expose a `Definition` implementing `Migration`. This applies to both handwritten migrations and files created by `make:migration`. Rebuild the application to include them. The generated list is passed to `with_migrations` in `bootstrap/app.rs`. The filename still determines the ledger name, and `--path` selects from the compiled migrations. For generation into a custom directory, register those entries with the application separately.

### Squashing Migrations {#squashing-migrations}

Run `schema:dump` to save MySQL table definitions, views, routines, and triggers with the migration ledger. It excludes application rows and resets table auto-increment counters. The MySQL 8.4 `mysqldump` and `mysql` clients must be on PATH.

For MariaDB, use `./artisan schema:dump --database=mariadb`. Export and restore use `mariadb-dump` and `mariadb` from MariaDB 11.4 or later, with TLS verification enabled. In the default `required` mode they use MariaDB’s automatic verification when the server and authentication plugin support it. For CA and hostname verification, configure `verify_identity` with the server CA. If a connection uses `verify_ca`, select `verify_identity` with the correct server name before running a snapshot command.

For PostgreSQL, `./artisan schema:dump --database=pgsql` uses `pg_dump` to export SQL definitions and the migration ledger. Restoration uses `psql` for SQL files and `pg_restore` for PostgreSQL archives. Use a `.sql` extension for a custom dump path and install the corresponding clients on PATH. If the connection has a direct endpoint, the tools use it, and the default filename uses the configured connection name.

For SQLite, run `./artisan schema:dump --database=sqlite`. The snapshot preserves tables, indexes, views, triggers, virtual tables, and migration history. It leaves out application rows, internal SQLite tables, and virtual-table shadow tables, which SQLite recreates. OpenRust uses its SQLite connection for export and restore, including restoration into an in-memory database. Restore runs in a framework-managed transaction. Custom snapshot scripts should leave transaction control to the framework.

```sh
./artisan schema:dump --database=mysql
./artisan migrate --database=mysql
```

The default file is `database/schema/<connection>-schema.sql`. Use `--database` to choose a connection and `--path` to write to a different file. `migrate` loads the snapshot when the ledger has no migration records and then runs pending compiled migrations. `--schema-path` selects a different input file. Pretend mode skips loading.

`--without-migration-data` exports only definitions, including the ledger table definition. Keep the ledger data when squashing migrations so the migrator can skip the migrations represented by the snapshot. `--prune` deletes compiled application migration source files from `database/migrations/` after the dump is written. Rebuild the application after pruning.

## Migration Structure

Implement `up` to apply a migration and `down` to reverse it. Use the supplied `MigrationContext` for schema changes and SQL so the migrator can collect the statements in pretend mode. Implement `get_connection` to select a different connection for the migration, or `should_run` to conditionally skip it.

`get_connection` receives the owning configuration repository and returns `DatabaseResult<Option<String>>`. The returned name is owned, allowing runtime configuration lookup without PHP globals or cross-application state. Return `Ok(None)` to use the command-selected connection. Passport's publishable migrations use this contract for `passport.connection`.

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct Definition;

impl Migration for Definition {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection.get_schema_builder().create("flights", |table| {
                table.id();
                table.string("name", 120);
                table.timestamps();
            }).await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection.get_schema_builder().drop_if_exists("flights").await
        })
    }
}
```

## Running Migrations

Create the SQLite database file before the first connection, or configure a MySQL database and credentials. `migrate` runs the pending migrations in file name order and records each successful migration in the configured ledger.

```sh
./artisan migrate --pretend
./artisan migrate
./artisan migrate:status
```

Use `--database` to select the ledger connection and `--step` to give each migration a separate batch. On an empty database, `--pretend` prepares the ledger and prints SQL without applying migrations or recording them. SQLite alterations that require rebuilding a table need live catalog metadata and report an error in pretend mode.

Production commands that change schema prompt for confirmation. Use `--force` for a deployment that has already approved those changes. MySQL and SQLite migrations can contain multiple DDL statements. If one fails, earlier successful statements may still be present. Inspect the failing migration before retrying.

### Prohibiting Destructive Commands {#prohibiting-destructive-commands}

Set the policy in a service provider's `boot` hook. This example prohibits `db:wipe`, `migrate:fresh`, `migrate:refresh`, `migrate:reset` and `migrate:rollback` in production. A prohibited command exits with status 1 before connecting to the database, even with `--force`. The policy applies to the current application. `migrate` and `migrate:status` remain available.

```rust
fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
    Box::pin(async move {
        use openrust::support::facades::Db;

        let production = app.environment(Some("production".into()))?;
        Db::prohibit_destructive_commands(bool::from(&production))?;
        Ok(())
    })
}
```

### Migration Transactions {#migration-transactions}

On PostgreSQL and SQL Server, each migration runs its `up` or `down` method in a transaction by default. Use the supplied `MigrationContext` to run Schema changes and SQL on that transaction. Migration listeners can use the transaction supplied by their event context. If the migration or a synchronous lifecycle listener fails before commit, its changes roll back and the ledger keeps its prior state.

Override `within_transaction()` in the migration’s `impl Migration` block for operations that require autocommit, such as [PostgreSQL online indexes](#postgresql-online-indexes). An opted-out migration can leave completed changes in place after a later failure. `--pretend` displays SQL without starting the migration transaction.

```rust
fn within_transaction(&self) -> bool {
    false
}
```

### Checking for Pending Migrations {#checking-pending-migrations}

Use `--pending` to list only pending migrations. Use `--pending=17` to make the command return 17 when that list is nonempty. An empty list returns 0. The explicit exit code must be between 0 and 255. A missing migration ledger returns 1. Use `--path` and `--database` to scope the check.

```sh
./artisan migrate:status --pending=17
```

Before the first pending migration, a migration whose `should_run` condition is false appears as `Skipped`. Subsequent unrecorded migrations appear as `Pending` because earlier migrations may change the database state used by their conditions.

### Reporting Migration Failures as Warnings {#graceful-migrations}

Use `--graceful` to report an execution error as a warning and return exit code 0. This covers migration and seeding errors. The failed migration remains pending, and any earlier successful migrations remain recorded. Production confirmation still applies.

```sh
./artisan migrate --graceful
```

### Excluding Pending Migrations {#excluding-pending-migrations}

Call `without_migrations` on the application’s migrator from a service provider to exclude named migrations from apply and pretend operations. Use the migration name or its `.rs` path. A later call replaces the list, and an empty slice clears it. Status continues to report the ledger and migration conditions, and rollback uses the recorded batch history.

```rust
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::migrations::Migrator,
    foundation::Application,
    support::ProviderResult,
};

fn configure_migrations(app: &Application) -> ProviderResult {
    let migrator = app.container().get::<Migrator>("migrator")?;
    migrator.without_migrations(&["database/migrations/0001_create_first.rs"]);
    Ok(())
}
```

### Creating a Missing Database {#creating-a-missing-database}

When the configured MySQL or PostgreSQL database is missing, `migrate` prompts before creating it. For unattended deployment, use `--force`. The selected write account must have permission to create the database. PostgreSQL uses the selected migration endpoint, including a configured direct endpoint, and temporarily connects to the `postgres` maintenance database.

For SQLite, `migrate` offers to create a missing database file at the configured path. For unattended runs, pass `--force`. Without this flag, `--no-interaction` leaves the file absent. The parent directory must already exist. Existing files and read-only connections are excluded from automatic creation.

```sh
./artisan migrate --database=sqlite --force
./artisan migrate --database=mysql --force
./artisan migrate --database=pgsql --force
```

### Isolating Migration Execution {#isolating-migration-execution}

Use `--isolated` to acquire an atomic lock in the default cache before running migrations. All deployment processes must share the cache backend and its key prefix. Redis can provide the lock before the first migration. The database cache driver requires its lock table to exist.

```sh
CACHE_STORE=redis ./artisan migrate --isolated
CACHE_STORE=redis ./artisan migrate --isolated=12
```

If another process holds the lock, the command reports that it is already running and exits with status 0. Pass `--isolated=12` to use status 12 on contention. The option takes an integer exit status. Lock or cache errors stop the command. The lock is released after execution and expires after one hour if the process terminates abruptly.

## Rolling Back Migrations

By default, `migrate:rollback` selects only the latest recorded batch on the write connection and rolls it back in reverse migration order. Use `--step` to select a number of migrations, or `--batch` to select a specific batch.

```sh
./artisan migrate:rollback --pretend
./artisan migrate:rollback --step=1
```

Use `migrate:reset` to roll back all migrations and `migrate:refresh` to roll them back and apply them again. When the ledger exists, `migrate:fresh` drops all tables on the selected database, including tables with other prefixes, and reruns migrations. Add `--drop-views` to drop views as well.

### Refreshing MySQL Migrations {#refreshing-mysql-migrations}

Run `migrate:refresh` to roll back applied migrations and run them again. A positive `--step` limits how many migrations roll back. When the migration ledger is empty, the migration phase loads the selected connection’s default schema snapshot if one exists. The database must be reachable before rollback begins.

```sh
./artisan migrate:refresh --database=mysql --step=1
```

### Rebuilding a Database {#rebuilding-mysql-with-fresh}

After any production confirmation, `migrate:fresh` can create a missing MySQL or PostgreSQL database with the selected write account. If the database exists and has no migration ledger, the command starts migrations without clearing its tables. Use `--schema-path` to select the snapshot to restore.

On PostgreSQL, `db:wipe` and the cleanup phase of `migrate:fresh` start from the schemas in the selected write connection’s effective `search_path`. The connection’s `dont_drop` list accepts physical table names or schema-qualified names and defaults to `["spatial_ref_sys"]`. Use `--drop-views` to also remove views and `--drop-types` to remove user-defined types and domains. PostgreSQL cascades can affect dependent objects in other schemas.

```sh
./artisan migrate:fresh --database=mysql --schema-path=database/schema/custom.sql
```

## Tables

Use `Schema::create("books", |table| { … }).await?` to create a table on the default connection. Use `Schema::connection("reporting")?` to select a named connection. An explicit connection exposes the same API through `get_schema_builder()`.

Use `table` to alter a table, `rename` to rename it and `drop_if_exists` for conditional removal. Use `has_table` and `get_tables` to inspect the database. Schema operations use the write connection.

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_catalog(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();

    schema.create("authors", |table| {
        table.id();
        table.string("name", 120);
    }).await?;

    schema.create("books", |table| {
        table.id();
        table.foreign_id("author_id").constrained().cascade_on_delete();
        table.string("title", 200);
        table.unique(&["author_id", "title"], None);
        table.timestamps();
    }).await?;

    schema.table("books", |table| {
        table.string("isbn", 20).nullable();
        table.index(&["isbn"], Some("books_isbn_index"));
    }).await?;

    assert!(schema.has_columns("books", &["title", "isbn"]).await?);
    assert!(schema.has_index("books", "books_isbn_index").await?);
    Ok(())
}
```

### MySQL Table Options {#mysql-table-options}

During `Schema::create`, use `engine`, `charset`, and `collation` to set the table options. MySQL uses a blueprint setting first and then the connection default for that option. `comment` can be used when creating or altering a table. Read the applied engine, collation and comment with `get_tables()`. Use `inno_db()` as shorthand for `engine("InnoDB")`.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_customers() -> DatabaseResult<()> {
    Schema::create("customers", |table| {
        table.inno_db();
        table.charset("utf8mb4");
        table.collation("utf8mb4_unicode_ci");
        table.comment("Customer records");
        table.id();
        table.string("code", 24).collation("utf8mb4_bin");
    })
    .await
}
```

Use column-level `charset` and `collation` to override text encoding or comparison rules. SQLite applies column collation and ignores the MySQL-specific table settings.

### Table and column comments {#database-comments}

Use `.comment(...)` for short, readable database descriptions, such as “Users”, “Username”, and “Email”. Include the actual values for status columns, such as “Status: 0=enabled, 1=disabled”, and units for amounts and numeric timestamps. The status below is an example, not a built-in users column.

```rust
use openrust::{
    database::{DatabaseResult, schema::Schema},
    translation::__,
};

async fn create_users() -> DatabaseResult<()> {
    Schema::create("users", |table| -> DatabaseResult<()> {
        table.comment(__("schema.tables.users", &Default::default())?);
        table.id().comment(__("schema.columns.id", &Default::default())?);
        table.string("name", None)
            .comment(__("schema.columns.username", &Default::default())?);
        table.string("email", None)
            .comment(__("schema.columns.email", &Default::default())?);
        table.integer("status").default(0)
            .comment(__("schema.columns.account_status", &Default::default())?);
        Ok(())
    }).await
}
```

For localization, reuse `__()` and framework grouped translations in JSON files. For example, put the following entries in `lang/zh-Hans/schema.json`:

```json
{
    "tables": { "users": "用户表" },
    "columns": {
        "id": "主键",
        "email": "邮箱",
        "username": "用户名",
        "account_status": "状态：0=开启，1=关闭"
    }
}
```

`app.locale` (`APP_LOCALE`) and `app.fallback_locale` select translations through the existing translator. Add `lang/ja/schema.json`, `lang/fr/schema.json`, or another locale to support more languages; no schema API changes are needed. Missing entries use the configured fallback; if both catalogs lack the key, the translator returns the key as the framework does. A plain `.comment("Email")` stores that literal text. Language identifiers follow the same normalization as other application translations.

The callback returns `DatabaseResult<()>` so translation errors can propagate with `?` before its DDL is executed. Existing callbacks returning `()` still work. The standard migration generator keeps `id()` and `timestamps()` skeleton; add comments and translations as needed. Timestamp helpers can be expanded into explicit columns when each column needs its own comment.

PostgreSQL, MySQL, and MariaDB store these comments. The current SQLite and SQL Server implementations do not store them. Each database object stores one comment, selected when the migration runs. Changing application locale or editing an executed create migration does not rewrite existing comments. Use a new migration with `table.comment(...)` for table comments. To modify a column through Schema, provide its complete definition with `.comment(...).change()`. For a PostgreSQL metadata-only update, use `MigrationContext::statement` with a trusted `COMMENT ON COLUMN` statement; this does not require a new schema API.

The initial application migrations follow the framework: `0001_01_01_000000_create_users_table`, `0001_01_01_000001_create_cache_table`, and `0001_01_01_000002_create_jobs_table`. The jobs migration creates and rolls back `jobs`, `job_batches`, and `failed_jobs` together. Application-specific migrations use normal timestamped names. Rust uses `.rs` files, registered migration types, asynchronous execution and `Result` in place of PHP’s anonymous classes and exceptions. Existing databases created with separate queue migrations must have their migration history reconciled after backup before using the combined migration; renaming source files alone is insufficient.

### Temporary Tables {#temporary-tables}

Acquire a schema session before using `Blueprint::temporary()`. Create the table through the session’s Schema Builder and run SQL through the same session. `close().await?` drops the managed temporary tables before returning the connection to the pool. If the session is dropped, cleanup retains the physical connection until it can be safely released.

```rust
use openrust::database::{DatabaseResult, Value, schema::Schema};

async fn staged_code() -> DatabaseResult<Value> {
    let mut session = Schema::session().await?;
    session.get_schema_builder().create("import_codes", |table| {
        table.temporary();
        table.string("code", 20);
    }).await?;
    session.statement("INSERT INTO import_codes (code) VALUES (?)", &["pending".into()]).await?;
    let rows = session.select("SELECT code FROM import_codes", &[]).await?;
    session.close().await?;
    Ok(rows[0].get_at(0).cloned().unwrap_or(Value::Null))
}
```

SQLite exposes its temporary table metadata on the same session. In MySQL, `has_table()` checks the session’s managed temporary tables. Use a session SQL statement such as `SHOW COLUMNS` to inspect them.

## Columns

A blueprint closure receives a mutable `Blueprint`. Column modifiers follow the column definition, such as `table.string("isbn", 20).nullable()`. Use `id()` for a generated primary key and `timestamps()` for nullable creation and update timestamps. Length, precision and scale are explicit Rust arguments.

Use `change` to specify a replacement column definition, including the modifiers to keep. `rename_column` and `drop_column` use native driver operations. `get_columns`, `get_column_listing` and `get_column_type` read column metadata from the catalog.

SQLite rebuilds the table for column and foreign key changes that require it. Run a rebuild through the connection's schema builder outside an active transaction. The rebuild owns its transaction and restores the connection's foreign key settings. MySQL DDL that implicitly commits also runs through the connection's schema builder.

### Additional Column Types {#additional-column-types}

Use `tinyText`, `year`, `ipAddress`, `mac_address` and `r#enum` to declare the corresponding columns. A year column with `use_current()` defaults to the database’s current year. MySQL stores native ENUM and SET values. SQLite enforces enum values with a CHECK constraint and returns an unsupported schema error for SET.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_events() -> DatabaseResult<()> {
    Schema::create("incoming_events", |table| {
        table.small_increments("id");
        table.r#enum("status", &["pending", "processed"]).default("pending");
        table.year("batch_year").use_current();
        table.ipAddress("source_ip").nullable();
        table.binary_with("digest", 32, true).nullable();
        table.timestamps_tz();
    })
    .await
}
```

`binary()` declares a blob. `binary_with(name, length, fixed)` uses MySQL BINARY or VARBINARY and SQLite BLOB. MySQL uses its native widths for `tiny_increments`, `small_increments`, `medium_increments` and the matching unsigned integer helpers. SQLite uses INTEGER. On both drivers, the `Tz` helpers use the corresponding native temporal types.

### PostgreSQL Search Vector Columns {#postgresql-tsvector-columns}

On PostgreSQL, use `tsvector` to declare a native search vector column. This example stores the vector generated from `body` and uses a GIN index over the stored column. Other drivers return an unsupported schema error.

```rust
table.text("body");
table
    .tsvector("search_terms")
    .stored_as("to_tsvector('english', body)");
table
    .index(&["search_terms"], Some("documents_search_terms"))
    .algorithm("gin");
```

### ULID Column Length {#ulid-column-length}

Use `ulid_with(name, length)` and `foreign_ulid_with(name, length)` to declare a different character width for imported identifiers. `ulid` and `foreign_ulid` keep the standard width of 26. These methods set the column width. A standard ULID still contains 26 characters.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_import_tables() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.ulid_with("id", 32).primary();
    }).await?;
    Schema::create("import_entries", |table| {
        table.id();
        table.foreign_ulid_with("import_id", 32)
            .constrained_on("imports", "id", None)
            .cascade_on_delete();
    }).await
}
```

### Inspecting SQLite Columns {#inspecting-sqlite-columns}

For a SQLite column declared as `VARCHAR(32)`, `get_column_type(..., false)` returns `varchar` and `get_column_type(..., true)` returns `varchar(32)`. Both return an empty string for a column declared without a type.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_title() -> DatabaseResult<(String, String)> {
    Ok((
        Schema::get_column_type("books", "title", false).await?,
        Schema::get_column_type("books", "title", true).await?,
    ))
}
```

Column metadata reports `auto_increment` when an integer primary key aliases the rowid and SQLite can generate its value. The `nullable` flag reflects whether the column can store a null. For example, an ordinary SQLite composite primary key can permit nulls unless constrained by `NOT NULL`, `STRICT`, or `WITHOUT ROWID`.

### Editing a Blueprint {#editing-a-blueprint}

Use `id_with("import_id")` to name an auto-incrementing primary key. It corresponds to `id("import_id")`. Use `id()` for the default name.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_imports() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.id_with("import_id");
        table.string("source", 32);
        table.text("temporary_notes");
        table.remove_column("temporary_notes");
    })
    .await
}
```

Use `remove_column()` inside a blueprint callback to discard a pending column addition or change. It removes modifiers declared on that column. Explicit indexes and constraints remain in the blueprint. Use `drop_column()` to drop an existing database column.

### Inspecting a Blueprint {#inspecting-a-blueprint}

In a schema callback, `get_table()` returns the supplied table reference before the framework applies the connection prefix. `creating()` distinguishes `Schema::create` from `Schema::table` and conditional alterations.

```rust
use openrust::database::schema::Blueprint;

fn prepare_status(table: &mut Blueprint) {
    if table.creating() {
        table.id();
    }

    if !table
        .get_columns()
        .any(|column| column.get_name() == "status")
    {
        table.string("status", 24).default("pending");
    }
}
```

Use `get_columns()`, `get_added_columns()`, and `get_changed_columns()` to read pending definitions in declaration order. These iterators borrow the blueprint. Use the Schema facade’s catalog methods to inspect columns already in the database.

### Resolving a Blueprint {#resolving-a-blueprint}

Set a resolver on a schema builder to add shared definitions before the operation’s callback runs. The resolver receives a mutable blueprint with the table reference, connection prefix and creation state already set.

```rust
use openrust::database::schema::Builder;

fn use_import_defaults(schema: &mut Builder<'_>) {
    schema.blueprint_resolver(|table| {
        if table.creating() {
            table.id();
            table.string("source", 32);
        }
    });
}
```

A schema builder keeps its own resolver. Register one on the builder used by the migration, transaction or session. Migration previews compile the same definitions into SQL.

### Reading the Connection Table Prefix {#reading-the-connection-table-prefix}

Use `Db::get_table_prefix()` for the default connection or `Db::connection(Some("archive"))?.get_table_prefix()` for a named connection. Inside a blueprint callback, `get_prefix()` reports the prefix of that blueprint’s connection.

```rust
use openrust::{database::DatabaseResult, support::facades::Db};

fn migration_prefixes() -> DatabaseResult<(String, String)> {
    let default = Db::get_table_prefix()?;
    let archive = Db::connection(Some("archive"))?;
    Ok((default, archive.get_table_prefix().to_owned()))
}
```

Pass logical names to Schema and the query builder. They apply the prefix automatically. Use the prefix when a migration needs to address a physical table in raw SQL.

### JSON Columns {#json-columns}

On MySQL, `json()` and `jsonb()` both declare `JSON`. On SQLite, both default to `TEXT`. Set `use_native_json` or `use_native_jsonb` to `true` in the SQLite connection configuration to declare `JSON` or `JSONB` respectively. The options are independent.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_documents() -> DatabaseResult<()> {
    Schema::create("documents", |table| {
        table.id();
        table.json("document");
        table.jsonb("snapshot");
    })
    .await
}
```

Declaring a SQLite column as `JSONB` does not encode values as binary JSON. Use SQLite’s `jsonb(...)` SQL function to store a binary value and `json(...)` to read it as JSON text.

### Generated Columns {#generated-columns}

Use `virtual_as` to compute a column when it is read or `stored_as` to store the computed result. Both take a trusted SQL expression. `virtual_as_json` and `stored_as_json` use an arrow-separated JSON path. A generated column must have a declared type, and the database computes its value from the other columns.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_invoice_lines() -> DatabaseResult<()> {
    Schema::create("invoice_lines", |table| {
        table.id();
        table.integer("quantity");
        table.integer("unit_cents");
        table.integer("total_cents").stored_as("quantity * unit_cents");
        table.json("metadata").nullable();
        table.string("currency", 3).virtual_as_json("metadata->currency").nullable();
    })
    .await
}
```

`get_columns()` exposes the generation mode and expression. MySQL and SQLite support both modes when creating tables. SQLite’s native ALTER can add a virtual column. Add a stored column when creating a table, or use a migration that explicitly rebuilds the table.

#### MySQL Generated Columns and Current Defaults {#mysql-generated-columns-and-current-defaults}

MySQL generated columns allow NULL by default. Use `nullable_with(false)` to require a value, or `nullable()` to allow NULL explicitly. Regular columns are required by default.

On MySQL, `use_current()` takes precedence over an explicit default on DATE, YEAR, DATETIME, and TIMESTAMP, regardless of modifier order. The database supplies the current date, year, or timestamp, using column precision for timestamps. Other column types keep their explicit default.

```rust
use openrust::database::schema::Blueprint;

fn define_import_metrics(table: &mut Blueprint) {
    table.id();
    table.integer("quantity").nullable();
    table.integer("doubled").virtual_as("quantity * 2");
    table.integer("total")
        .stored_as("COALESCE(quantity, 0)")
        .nullable_with(false);
    table.timestamp("recorded_at", 3)
        .use_current()
        .default("2000-01-01 00:00:00");
}
```

### Spatial Columns {#spatial-columns}

Use `geometry` or `geography` to declare a spatial column. The optional subtype selects a MySQL type such as `point` or `multipolygon`. The default SRID is 0 for `geometry` and 4326 for `geography`. Pass `Some(0)` to omit the SRID restriction.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_places() -> DatabaseResult<()> {
    Schema::create("places", |table| {
        table.id();
        table.geography("location", Some("point"), None).spatial_index();
        table.geometry("outline", Some("polygon"), Some(4326)).nullable();
    })
    .await
}
```

MySQL enforces the declared subtype and any nonzero SRID. A non-null spatial column can declare an index with `spatial_index()`. SQLite declares a `geometry` column and supports storing binary values. Spatial functions and spatial indexes require a spatial-capable database.

### Vector Columns {#vector-columns}

On PostgreSQL, enable the installed `pgvector` extension before declaring a vector column. Pass `None` to use the default schema or `Some(schema)` to place the extension explicitly. The extension must be visible on the connection’s search path. MariaDB 11.7 and later provide vector columns natively.

```rust
use openrust::support::facades::Schema;

Schema::ensure_vector_extension_exists(None).await?;
```

```rust
use openrust::support::facades::Schema;

Schema::create("documents", |table| {
    table.id();
    table.vector("embedding", Some(1536)).index();
}).await?;
```

Both drivers accept an explicit dimension. PostgreSQL also accepts `None` for a column without a fixed dimension. Use a fixed dimension for the default cosine index. MariaDB requires an explicit dimension and a non-null indexed column.

Calling `index()` on a vector column creates a cosine index. PostgreSQL uses HNSW with `vector_cosine_ops`. MariaDB uses `VECTOR INDEX` with `M=6` and cosine distance. Use `vector_index("embedding", Some("semantic_idx"))` to choose a name. On PostgreSQL, `algorithm("ivfflat")` and `operator_class("vector_l2_ops")` select another index method and distance operator class. The operator class must match the distance expression used by the query.

Drop a named index with `drop_vector_index("semantic_idx")` or the conventional index with `drop_vector_index_columns(&["embedding"])`. See [vector similarity queries](./queries.md#vector-similarity-clauses) for querying the column.

On MariaDB, `operator_class("M=8 DISTANCE=euclidean")` sets vector index options. `M` accepts 3 through 200, and `DISTANCE` accepts `cosine` or `euclidean`. The Query Builder’s vector search methods use cosine distance.

To build a PostgreSQL vector index while accepting writes, see [PostgreSQL online indexes](#postgresql-online-indexes).

### MySQL Column Modifiers

Use `invisible()` to omit a column from `SELECT *` while keeping it available by name. `use_current_on_update()` updates a timestamp or datetime when another value in the row changes. Add `use_current()` if the insert default should also use the current time. Use `first()` or `after("column")` when adding or changing columns on an existing MySQL table. On table creation, declare columns in the desired order.

```rust
Schema::table("books", |table| {
    table.string("internal_note", 120).nullable().invisible();
    table.timestamp("reviewed_at", 6).use_current().use_current_on_update();
    table.string("edition", 20).nullable().after("title");
}).await?;
```

## Indexes

Use `index`, `unique` or `primary` with a column slice. Pass an explicit name or `None` for the conventional name. Use `get_indexes`, `has_index` and `rename_index` to inspect or rename an index. MySQL supports full-text indexes on compatible columns.

### Foreign Key Constraints

Use `foreign_id("author_id").constrained()` for a reference to `authors.id`. Use `constrained_on` to name the referenced table and column explicitly. Apply column modifiers before defining the foreign key, and referential actions such as `cascade_on_delete()` afterward. Use `get_foreign_keys` to inspect constraints. SQLite exposes the constrained columns in its catalog, and `drop_foreign_columns` can remove a table-level constraint by those columns.

#### Default Column Length and Time Precision {#default-column-length-and-time-precision}

`Schema::default_string_length` and `Schema::default_time_precision` set defaults for subsequent definitions in the current application. The initial defaults are 255 characters and 0 fractional seconds digits. Pass `None` to an individual column to use the application default, or pass an explicit length or precision.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_entries() -> DatabaseResult<()> {
    Schema::default_string_length(120)?;
    Schema::default_time_precision(Some(3))?;

    Schema::create("entries", |table| {
        table.id();
        table.string("label", None);
        table.timestamps();
        table.soft_deletes_datetime(Some("removed_at"), 6);
    })
    .await
}
```

`timestamps()` and `timestamps_tz()` use the configured precision. Use `timestamps_with` or `timestamps_tz_with` for an explicit precision. `datetimes` creates nullable DATETIME columns for `created_at` and `updated_at`. `soft_deletes_datetime` creates the nullable DATETIME soft-delete column. SQLite uses its native column declarations.

#### MySQL Column Lengths and Update Expressions {#mysql-column-lengths-and-update-expressions}

On MySQL, use `char(name, 0)` for an empty fixed-width character column. `CHAR` allows up to 255 characters. A zero length in `binary_with` behaves like `binary` and creates a BLOB. Positive lengths create BINARY or VARBINARY depending on the `fixed` argument.

Use `on_update` with a trusted SQL expression for a TIMESTAMP or DATETIME column. Match the expression’s fractional precision to the column. When both update modifiers are specified, `use_current_on_update` uses the column’s precision and takes precedence.

```rust
use openrust::database::{query::Expression, schema::Blueprint};

fn define_import_status(table: &mut Blueprint) {
    table.id();
    table.char("flag", 0).nullable();
    table.binary_with("payload", 0, false).nullable();
    table.date_time("changed_at", 3)
        .use_current()
        .on_update(Expression::new("NOW(3)"));
}
```

#### Floating-Point Precision and Raw Columns {#float-precision-and-raw-columns}

`float()` uses default precision of 53 bits. Use `float_with(name, precision)` to set it explicitly. On MySQL, precision 1–24 selects FLOAT and 25–53 selects DOUBLE. Use precision 0 for a native FLOAT declaration without a precision argument. SQLite uses its native floating-point behavior regardless of the requested precision.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_measurements() -> DatabaseResult<()> {
    Schema::create("measurements", |table| {
        table.id();
        table.float("value");
        table.float_with("device_value", 24);
        table.raw_column("sample_count", "smallint").default(0);
    })
    .await
}
```

Pass a trusted SQL column definition to `raw_column(name, definition)` in a migration. Schema quotes the column name and appends the usual modifiers. The database interprets the supplied definition.

#### Column Groups and Drop Aliases {#column-groups-and-drop-aliases}

`morphs()` and `nullable_morphs()` use the application’s configured morph key type. Use `numeric_morphs`, `uuid_morphs`, `ulid_morphs`, or their nullable variants to select the ID type explicitly. For an explicit index name or MySQL column position, pass those options to the typed functions, `morphs_with`, or `nullable_morphs`. Use `Schema::drop_columns(table, columns)` to remove columns without a Blueprint callback.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_attachments() -> DatabaseResult<()> {
    Schema::create("attachments", |table| {
        table.id();
        table.nullable_morphs("attachable", Some("attachments_attachable"), None);
        table.remember_token();
        table.nullable_timestamps();
    })
    .await
}

async fn remove_attachment_metadata() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.drop_morphs("attachable", Some("attachments_attachable"));
        table.drop_remember_token();
        table.drop_timestamps();
    })
    .await
}

async fn remove_legacy_columns() -> DatabaseResult<()> {
    Schema::drop_columns("attachments", &["legacy_name", "legacy_rank"]).await
}
```

`drop_morphs` drops the composite index before removing both columns. Pass the customized name if the index was named explicitly. `drop_constrained_foreign_id` drops the foreign key and then the column. `drop_soft_deletes(None)` and `drop_soft_deletes_tz(None)` remove the default `deleted_at` column, while `Some(name)` selects a different column.

To add several columns in order after an existing MySQL column, use `table.after("title", |table| { … })` inside `Schema::table`. PostgreSQL, SQLite and SQL Server ignore `first()` and `after()` positioning hints; new columns retain the database’s normal append order.

#### Default Expressions {#default-expressions}

Pass a trusted SQL expression to `default()` using `Db::raw()` or `Expression::new()`. The database evaluates the expression when an insert omits the column. Passing an ordinary string stores it as a literal default.

```rust
use openrust::{database::DatabaseResult, support::facades::{Db, Schema}};

async fn create_flights() -> DatabaseResult<()> {
    Schema::create("flights", |table| {
        table.id();
        table.json("movies").default(Db::raw("(JSON_ARRAY())"));
    })
    .await
}
```

Default expressions cannot contain parameter bindings. Supply the SQL syntax required by your database, including parentheses where required. The database checks whether the expression is valid for the column type and schema operation.

#### MySQL Instant Column Changes and Locking {#instant-column-operations}

Use `instant()` when adding a column or changing its definition with `change()`. Set the column operation's lock mode to `none`, `shared`, `exclusive`, or `default` with `lock()`. MySQL checks whether the operation supports the requested algorithm and lock combination. If it rejects them, the error is returned without retrying with a different algorithm.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn prepare_imports() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.id().from(500);
        table.string("name", 100);
    })
    .await?;

    Schema::table("imports", |table| {
        table.string("note", 100).nullable().instant().lock("default");
    })
    .await
}
```

Use `from()` or `starting_value()` to set the starting value of an auto-increment column on MySQL. MySQL determines the next available value using the existing rows. SQLite uses its native sequence and rejects the MySQL online DDL modifiers.

#### Auto-Incrementing Columns and Explicit Primary Keys {#auto-incrementing-columns-and-explicit-primary-keys}

Use `integer_increments("id")` as an alias for `increments("id")`. Both create an unsigned integer. The `auto_increment()` modifier preserves the signedness of the column type. On MySQL, a table-level `primary` declaration supplies the entire primary key. InnoDB requires the auto-increment column to be the leading column in an index.

```rust
use openrust::database::schema::Blueprint;

fn define_import_entries(table: &mut Blueprint) {
    table.integer("entry_id").auto_increment().starting_value(500);
    table.integer("import_id");
    table.primary(&["entry_id", "import_id"], None);
}
```

#### Conditional Schema Changes {#conditional-schema-changes}

Use `when_table_has_column`, `when_table_doesnt_have_column`, `when_table_has_index`, and `when_table_doesnt_have_index` to run a blueprint only when its catalog check matches. The check and subsequent schema statement are separate operations.

```rust
use openrust::{
    database::{DatabaseResult, schema::KeySelector},
    support::facades::Schema,
};

async fn prepare_imports() -> DatabaseResult<()> {
    Schema::when_table_doesnt_have_column("imports", "status", |table| {
        table.string("status", 24).default("pending");
    })
    .await?;

    Schema::when_table_doesnt_have_index(
        "imports",
        KeySelector::Columns(&["status", "source"]),
        |table| {
            table.unique(&["status", "source"], Some("imports_status_source"));
        },
        Some("unique"),
    )
    .await
}
```

`has_index_with` and both index callbacks accept a `KeySelector` by name or exact column order, with an optional type such as `primary`, `unique`, or `btree`. `has_foreign_key` accepts the same selector on the local columns.

#### Creating and Dropping Databases {#creating-and-dropping-databases}

`create_database` creates a database on the selected MySQL server using the connection’s configured character set and collation. It returns an error if the name already exists. `drop_database_if_exists` removes the named database and succeeds if it is already absent.

On PostgreSQL, both methods execute database-level DDL outside transactions. Creation uses the connection’s configured character set as the database encoding when supplied. Connect to an existing database before creating another one. To drop a database, connect to a different database on the same server.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_reporting_database() -> DatabaseResult<()> {
    Schema::create_database("reporting_staging").await?;
    Ok(())
}

async fn drop_reporting_database() -> DatabaseResult<()> {
    Schema::drop_database_if_exists("reporting_staging").await?;
    Ok(())
}
```

On SQLite, supply a file path. Creation fails if the path already exists, preserving the existing database. Close connections to the file before removing it.

#### Dropping All Tables or Views {#dropping-all-tables-or-views}

`drop_all_tables()` removes permanent tables from the selected connection’s current schema. `drop_all_views()` removes views from that schema. These methods apply to all objects of the requested kind, even when a table prefix is configured. Use a named connection when targeting a separate database.

PostgreSQL follows the [same schema scope and table exclusions as `db:wipe`](#rebuilding-mysql-with-fresh). Use `drop_all_types()` to drop its user-defined types and domains.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn clear_reporting_schema() -> DatabaseResult<()> {
    let mut schema = Schema::connection("reporting_staging")?;
    schema.drop_all_views().await?;
    schema.drop_all_tables().await
}
```

The table operation leaves views in place. A view that references a removed table may need to be dropped or recreated. MySQL table removal temporarily disables foreign key checks on the write session and restores the previous setting.

#### Inspecting Tables {#inspecting-tables}

`get_tables(None)` lists tables across visible user schemas. Pass `Some(name)` to limit the query to one schema or use `get_tables_in(&[...])` for several. Use an explicit schema when checking the result of a schema-scoped wipe.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_tables(schema: &str) -> DatabaseResult<()> {
    let names = Schema::get_table_listing(Some(schema)).await?;
    for name in names {
        println!("{name}");
    }
    Ok(())
}
```

`get_table_listing` returns qualified names. Use `get_table_listing_with(schema, false)` for names without a schema prefix. Table entries contain catalog names and, when supplied by the database, size and storage metadata.

#### Parsing Table References {#parsing-table-references}

`parse_schema_and_table(reference, fallback)` splits a logical table reference before the connection adds its table prefix. A qualified reference keeps its explicit schema. For a bare table name, use `SchemaFallback::None`, `SchemaFallback::Current`, or `SchemaFallback::Named(name)`. Use `Schema::connection(name)` to select a connection. Table references accept at most two nonempty parts.

```rust
use openrust::{
    database::{DatabaseResult, schema::SchemaFallback},
    support::facades::Schema,
};

async fn list_tables_in_same_schema(reference: &str) -> DatabaseResult<Vec<String>> {
    let (schema, _) = Schema::parse_schema_and_table(reference, SchemaFallback::Current)?;
    Schema::get_table_listing(schema.as_deref()).await
}
```

#### Inspecting a Column {#inspecting-a-column}

`Schema::get_column` returns the metadata for one column. Column names are compared case-insensitively. Rust returns `None` when the column is absent, and database errors propagate through `DatabaseResult`.

On PostgreSQL, column inspection includes identity and serial flags, primary-key membership, and generated expressions. An unqualified table name uses the server’s current schema. Pass a qualified name to inspect a table in another schema.

```rust
use openrust::{
    database::{DatabaseResult, schema::Column},
    support::facades::Schema,
};

async fn inspect_title() -> DatabaseResult<Option<Column>> {
    Schema::get_column("posts", "title").await
}
```

#### Inspecting Schemas and Views {#inspecting-schemas-and-views}

`get_schemas()` lists the visible schemas and marks the selected one. SQLite also reports each schema’s file path. `get_views(None)` returns views from all visible user schemas on MySQL or all schemas on the SQLite write session. Use `get_views(Some(name))` for one schema or `get_views_in(&[...])` for an explicit list. `get_current_schema_name()` and `get_current_schema_listing()` use the configured write connection. MySQL uses its configured database, and SQLite uses `main`. These two methods read connection settings without issuing SQL.

On PostgreSQL, `get_current_schema_listing()` returns the configured `search_path` in order, with `$user` resolved to the selected endpoint’s username. Use a comma-separated string or an array of schema names. `get_current_schema_name()` returns the first entry. The database catalog methods query the selected writer, and `get_schemas()` marks the schema currently in use by the server.

`Schema::get_types(None)` lists PostgreSQL types across user schemas. Pass `Some(schema)` or use `Schema::get_types_in(&[...])` to limit the scope. Each result provides a qualified name, type, category, and an `implicit` flag for generated arrays and multiranges. Other drivers return an unsupported-schema error.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_status_view() -> DatabaseResult<()> {
    if Schema::has_view("status_report").await? {
        let current = Schema::get_current_schema_name()?;
        for view in Schema::get_views(Some(&current)).await? {
            println!("{}", view.schema_qualified_name);
        }
    }
    Ok(())
}
```

`has_view("name")` checks the current schema. Use a qualified name to check another schema. The connection’s table prefix applies to `has_view`. View entries report catalog names and may include a definition.

#### Renaming and Changing a Column {#renaming-and-changing-a-column}

On MySQL, combine `rename_to()` with `change()` to rename a column and set its new definition in one ALTER statement. The definition starts with the current name. Include the modifiers you want to keep.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn revise_import_labels() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.string("old_label", 100).nullable().rename_to("label").change();
    })
    .await
}
```

For a rename alone, use `rename_column`. On SQLite, use `rename_column` for the rename and a separate `change()` for the new definition.

#### Changing Columns and Their Indexes {#changing-columns-and-their-indexes}

A plain `change()` preserves indexes and foreign keys. Use `unique_with(false)` or `index_with(false)` with `change()` to remove an index by its conventional name. When renaming the column during the same operation, removal uses its old name and a new index uses the new column.

Use `index_with("name")`, `unique_with("name")`, `full_text_with("name")`, or `spatial_index_with("name")` to add a named column index. To remove a custom-named index, use the corresponding table-level drop method. MySQL requires an index that backs a foreign key until you explicitly drop the constraint or provide another suitable index.

```rust
use openrust::database::schema::Blueprint;

fn change_region_code(table: &mut Blueprint) {
    table.string("postal_code", 30)
        .unique_with(false)
        .index_with("region_lookup")
        .rename_to("region_code")
        .change();
}
```

#### Replacing a MySQL Primary Key {#replacing-a-mysql-primary-key}

Call `drop_primary()` immediately before `primary(...)` in the same MySQL table callback. OpenRust emits one ALTER TABLE statement. Keep an auto-increment column first in a suitable index, such as the new primary key in this example. Add index options such as `inplace()` or `lock("none")` to the replacement declaration when the engine supports them.

```rust
use openrust::database::schema::Blueprint;

fn replace_import_key(table: &mut Blueprint) {
    table.drop_primary();
    table.primary(&["id", "tenant_id"], None);
}
```

#### Adding an Auto-Increment Column and a Primary Key {#adding-an-auto-increment-column-and-a-primary-key}

On an existing MySQL table, declare the new auto-increment column immediately before its primary key in the same callback. OpenRust emits one ALTER TABLE statement. Keep the auto-increment column first in the primary key when it has no other supporting index. To replace an existing primary key, call `drop_primary()` immediately before the column declaration.

```rust
use openrust::database::schema::Blueprint;

fn add_import_key(table: &mut Blueprint) {
    table.big_increments("id");
    table.primary(&["id", "tenant_id"], None);
}
```

Column and index lock settings must agree when both are specified. MySQL validates the chosen algorithm and lock mode for the combined operation.

#### Dropping Indexes by Column List {#dropping-indexes-by-columns}

Pass the ordered column list to `drop_index_columns`, `drop_unique_columns`, `drop_full_text_columns`, or `drop_spatial_index_columns` to drop an index created with the default name for its kind. For a custom name, use the corresponding method that takes the name.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn remove_import_lookup() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.drop_index_columns(&["status", "created_at"]);
    })
    .await
}
```

#### Inspecting SQLite Primary Keys {#sqlite-primary-indexes}

On SQLite, `get_indexes()` reports the native name for a primary key with a backing index. A rowid primary key without a backing index appears as `primary`. An ordinary index named `primary` can appear alongside it.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn primary_columns() -> DatabaseResult<Vec<Option<String>>> {
    let indexes = Schema::get_indexes("deliveries").await?;
    Ok(indexes
        .into_iter()
        .find(|index| index.primary)
        .map(|index| index.columns)
        .unwrap_or_default())
}
```

Use the `primary` flag to identify the constraint and read its columns in key order. `has_index("deliveries", "primary")` also recognizes a primary key with a native index name.

#### Raw Indexes {#raw-indexes}

`raw_index(expression, name)` adds a named index using a database-specific SQL key list. MySQL requires an extra pair of parentheses around each functional key part. SQLite accepts `lower(label)` directly. Supply SQL from your migration code. Drop the index with `drop_index(name)`.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn index_labels() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.raw_index("(lower(label))", "imports_lower_label");
    })
    .await
}
```

#### Index Methods and Online Column Drops {#index-methods-and-online-column-drops}

On MySQL, use `algorithm("btree")` or `algorithm("hash")` to request an index method. `inplace()` selects the DDL algorithm for the operation. MySQL decides whether the specified options are supported by the storage engine.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn revise_imports() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.index(&["status"], Some("imports_status"))
            .algorithm("btree")
            .inplace()
            .lock("none");
        table.drop_column(&["legacy_code", "legacy_note"])
            .instant()
            .lock("default");
    })
    .await
}
```

Pass multiple names to `drop_column` to drop them in one MySQL ALTER statement. Chain `instant()` or `lock()` to request an online DDL option. If the engine rejects the statement, the columns in that statement remain in place. Earlier statements in the same blueprint may already have completed.

#### Changing PostgreSQL Columns {#postgresql-column-changes}

Use `change()` to declare the column’s new type, nullability, default and comment. Restate the modifiers you want to keep. Existing indexes remain in place unless you request an index change. If PostgreSQL needs an explicit conversion, pass a trusted SQL expression with `using()`. The expression applies to existing values. If an old default cannot be cast to the new type, remove it before the type change and declare the desired default on the new definition.

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext, query::Expression};

async fn update_amount(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.get_schema_builder().table("finance.fixture", |table| {
        table.decimal("amount", 7, 2)
            .using(Expression::new("amount::numeric / 100"))
            .default(1)
            .nullable()
            .comment("owner's amount")
            .change();
    }).await
}
```

#### PostgreSQL Generated and Identity Columns {#postgresql-generated-and-identity}

Use `stored_as()` for a stored generated column on PostgreSQL 12 or later, and `virtual_as()` for a virtual generated column on PostgreSQL 18 or later. On PostgreSQL 13 or later, call `drop_expression().change()` on a stored generated column to keep its current values as ordinary data. PostgreSQL rejects dropping a virtual expression, and the Schema Builder rejects replacing a generated expression through `change()`.

Use `generated_as()` for `BY DEFAULT` identity or chain `always()` to require generated values. With `generated_as_with(Expression)`, pass trusted sequence options such as `start with 10`. Explicit identity alone leaves the primary key definition to the Blueprint. On `change()`, explicit identity replaces the sequence using the new definition. Omit identity modifiers to remove identity.

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext, query::Expression};

async fn create_generated_table(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.get_schema_builder().create("finance.fixture", |table| {
        table.bigInteger("seq")
            .generated_as_with(Expression::new("start with 10"))
            .always();
        table.integer("base");
        table.integer("saved").stored_as("base * 2");
        table.integer("computed").virtual_as("base + 1");
    }).await
}
```

#### PostgreSQL JSON Selectors in Generated Columns {#postgresql-generated-json-selectors}

Use `stored_as_json()` or `virtual_as_json()` with `->` for nested keys and brackets for array offsets. PostgreSQL extracts the final path element as text. Missing paths and JSON null yield SQL NULL, so declare `nullable()` if the path may not contain a value. A path element like `01` is an object key, while `[-1]` selects the last array element. Stored and virtual columns follow the [PostgreSQL version requirements above](#postgresql-generated-and-identity).

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn create_json_columns(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.get_schema_builder().create("finance.fixture", |table| {
        table.integer("id");
        table.jsonb("payload");
        table.string("label", 80).stored_as_json("payload->items[0]->label").nullable().index();
        table.string("locale", 20).virtual_as_json("payload->settings->locale").nullable();
    }).await
}
```

#### PostgreSQL Index Methods and Renaming {#postgresql-index-methods}

Use `algorithm("brin")` to choose a PostgreSQL index access method. The server checks the method and whether it supports the indexed columns or expressions. You can combine it with `online()`. `rename_index()` inherits the schema from a schema-qualified table name. You can explicitly qualify the old index name. If both the table and the old index name are unqualified, PostgreSQL resolves the old index name through `search_path`. The new name stays in the existing index schema.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn index_references() -> DatabaseResult<()> {
    Schema::table("archive.documents", |table| {
        table.index(&["external_id"], Some("documents_reference_idx")).algorithm("brin");
        table.rename_index("documents_reference_idx", "documents_reference_brin");
    })
    .await
}
```

#### PostgreSQL Unique Constraints {#postgresql-unique-constraints}

Use `unique()` to create a named PostgreSQL constraint. On PostgreSQL 15 and later, `nulls_not_distinct(true)` treats NULL values as equal. `nulls_not_distinct(false)` explicitly keeps them distinct. Use `deferrable(true).initially_immediate(false)` to check uniqueness at commit. With `online()` or an explicit `algorithm("btree")`, the builder creates the supporting index before attaching it to the constraint. `drop_unique()` removes the constraint and its index. It can also remove a standalone unique index created by earlier OpenRust migrations on the same table.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn require_unique_reference() -> DatabaseResult<()> {
    Schema::table("documents", |table| {
        table
            .unique(&["external_id"], Some("documents_external_unique"))
            .nulls_not_distinct(true)
            .deferrable(true)
            .initially_immediate(false);
    })
    .await
}
```

#### PostgreSQL Online Indexes {#postgresql-online-indexes}

Call `online()` on an ordinary, unique, full-text, or vector index to use PostgreSQL’s `CREATE INDEX CONCURRENTLY`. The index can wait for existing transactions while other sessions continue writing.

```rust
use openrust::support::facades::Schema;

Schema::table("documents", |table| {
    table.vector_index("embedding", Some("documents_vector_search")).online();
}).await?;
```

Inside a migration, override [`within_transaction()`](#migration-transactions) to return `false`. Use the connection’s Schema builder. A blueprint containing an online index runs its statements individually in autocommit mode. Keep that blueprint separate from changes you want to apply atomically. An active framework-managed transaction rejects the blueprint before executing any of its statements. A failed concurrent build can leave an invalid index, which you can drop before retrying.

#### Changing and Dropping SQL Server Columns {#sql-server-column-changes}

On SQL Server, `change()` removes the column’s current default constraint before altering its type and nullability. When you supply `default()`, the builder applies the replacement in a separate `ADD DEFAULT … FOR …` statement in the same migration. Leaving out `default()` removes the old default.

When `drop_column()` removes columns, the builder first removes their default constraints and column-level checks. Remove any table-level constraints that depend on those columns before dropping them.

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn update_columns(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.get_schema_builder().table("finance.fixture", |table| {
        table.string("label", Some(20)).nullable().change();
        table.drop_column(&["score"]);
    }).await
}
```

#### Renaming SQL Server Tables, Columns and Indexes {#sql-server-renaming}

On SQL Server, `rename()`, `rename_column()` and `rename_index()` use `sys.sp_rename`. A table rename keeps the current Schema. The connection’s table prefix applies to both table names. An index retains its indexed columns and data. Update application code and stored SQL that still refer to the old name.

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn rename_fixture(cx: &mut MigrationContext) -> DatabaseResult<()> {
    let mut schema = cx.get_schema_builder();
    schema.table("finance.fixture", |table| {
        table.rename_column("score", "score_new");
        table.rename_index("ix_fixture", "ix_new");
    }).await?;
    schema.rename("finance.fixture", "finance.archive").await
}
```

#### SQL Server Computed Columns {#sql-server-computed-columns}

Use `computed(name, expression)` with a trusted SQL expression. SQL Server infers the column type. Add `persisted()` to store the computed value. For an existing computed column, `persisted().change()` stores the value and `change()` alone switches back to recomputing on read. Both changes use the existing expression. SQL Server checks whether that expression can be persisted.

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn persist_total(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.get_schema_builder().table("finance.fixture", |table| {
        table.computed("twice", "quantity * 2").persisted().change();
    }).await
}
```

#### SQL Server Primary Key Constraints {#sql-server-primary-key-constraints}

Use `primary(columns, Some(name))` to create a named primary key on a new or existing table. Use `drop_primary_named(name)` to remove it. Supply ordered columns to `drop_primary_columns()` if the key uses the conventional name. The no-argument `drop_primary()` looks up the current primary key on the selected table. SQL Server rejects the operation while a foreign key references that primary key.

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn remove_primary_key(cx: &mut MigrationContext) -> DatabaseResult<()> {
    let mut schema = cx.get_schema_builder();
    schema.table("finance.child", |table| table.drop_foreign("fixture_foreign")).await?;
    schema.table("finance.fixture", |table| table.drop_primary_named("fixture_key")).await
}
```

#### SQL Server Online Indexes {#sql-server-online-indexes}

On SQL Server editions that support online index operations, call `online()` on `index()` or `unique()` to use `WITH (ONLINE = ON)`. Keep the default [migration transaction](#migration-transactions). Use `drop_foreign(name)` to remove a foreign key by its constraint name.

```rust
schema.table("dbo.fixture_entries", |table| {
    table.index(&["score"], Some("fixture_search")).online();
    table.unique(&["parent_id"], Some("fixture_unique")).online();
    table.drop_foreign("fixture_parent");
}).await?;
```

#### Foreign IDs from Models {#foreign-ids-from-models}

Use `foreign_id_for::<Model>(None)` to let an Eloquent model supply the reference column name. `constrained()` uses the model’s table and primary key, including custom names. Pass `Some(name)` to use a different column name in the referencing table.

In this example, `PhotoAsset` uses a UUID primary key named `photo_code` in the `photo_assets` table.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn add_photo_reference() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.foreign_id_for::<PhotoAsset>(Some("photo_ref"))
            .nullable()
            .constrained()
            .null_on_delete();
    })
    .await
}

async fn drop_photo_reference() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.drop_constrained_foreign_id_for::<PhotoAsset>(Some("photo_ref"));
    })
    .await
}
```

Model IDs default to an unsigned big integer. A model that generates UUIDs or ULIDs for its primary key selects the corresponding column type. Use `foreign_uuid_for` and `foreign_ulid_for` to select a format explicitly, or override `Model::get_key_type` for manually assigned IDs. Use `drop_foreign_id_for` for a column that has no constraint.

#### Naming a Foreign Key {#naming-foreign-keys}

Use `constrained_with(table, column, index_name)` to override individual parts of a foreign key. An omitted table or column keeps the value supplied by the model. Without a model, the referenced column defaults to `id`, and the table name is inferred by pluralizing the part of the local column name before the last `_<referenced column>`. Pass an explicit name to `drop_foreign` when removing a named constraint.

```rust
Schema::table("attachments", |table| {
    table.foreign_id_for::<PhotoAsset>(Some("photo_ref"))
        .nullable()
        .constrained_with(None, None, Some("attachments_photo"))
        .null_on_delete();
}).await?;
```

#### SQLite Inline Constraints {#sqlite-inline-constraints}

Changing a SQLite column preserves its existing inline primary key, UNIQUE, CHECK and REFERENCES clauses. The rebuild also preserves the auto-increment sequence when the rebuilt table retains its auto-increment key. The column definition passed to `change()` controls the type, nullability and default.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn relax_owner_reference() -> DatabaseResult<()> {
    Schema::table("orders", |table| {
        table.drop_foreign_columns(&["owner_id"]);
        table.integer("owner_id").nullable().change();
    })
    .await
}
```

Use `drop_foreign_columns` for a foreign key’s local columns, or `drop_foreign` for an explicitly named constraint. Both forms can remove inline REFERENCES or table-level foreign keys. Use `drop_primary` to remove an inline or table-level primary key. Other inline constraints remain in force.

#### PostgreSQL Foreign Keys {#postgresql-foreign-keys}

Use qualified table names to add a foreign key across schemas. By default, PostgreSQL checks a new constraint against existing rows. Call `not_valid()` to skip that scan while still checking new rows and changes to the foreign key. Call `deferrable(true).initially_immediate(false)` to check at transaction commit. Use `initially_immediate(true)` to check at the end of each statement by default. Drop a key with `drop_foreign(name)` or `drop_foreign_columns(&[...])` for a conventionally named key.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn constrain_books() -> DatabaseResult<()> {
    Schema::table("library.books", |table| {
        table
            .foreign(&["author_id"], Some("books_author_foreign"))
            .references("id")
            .on("authors.records")
            .deferrable(true)
            .initially_immediate(false)
            .not_valid();
    })
    .await
}
```

#### Inspecting Foreign Keys {#inspecting-foreign-keys}

Use `get_foreign_keys` to inspect the local columns, target schema and table, target columns, and update and delete actions. A composite key’s columns are returned in constraint order.

On PostgreSQL, the catalog reports the constraint name and the referenced schema. Local and referenced columns are paired in constraint order, including compound keys that target a different schema.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn author_schema() -> DatabaseResult<Option<String>> {
    Ok(Schema::get_foreign_keys("books")
        .await?
        .into_iter()
        .find(|key| key.columns == ["author_id"])
        .and_then(|key| key.foreign_schema))
}
```

SQLite reports the containing schema, such as `main` or an attached database name. Its catalog leaves the constraint name unset. When the declaration omits the referenced column names, the corresponding entries are `None` and SQLite uses the referenced table’s primary key.

#### Toggling Foreign Key Constraints {#toggling-foreign-key-constraints}

Use `Schema::without_foreign_key_constraints` to run work on a single write session with foreign key checks disabled. Use the supplied session for SQL and its `get_schema_builder()` for schema operations. The session restores its original setting on exit.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn import_order() -> DatabaseResult<()> {
    Schema::without_foreign_key_constraints(|session| {
        Box::pin(async move {
            session
                .statement(
                    "INSERT INTO orders (id, customer_id) VALUES (?, ?)",
                    &[1.into(), 42.into()],
                )
                .await?;
            Ok(())
        })
    })
    .await
}
```

For manual control, acquire a write session with `Schema::session().await?` or the named builder’s `session().await?`, then call `disable_foreign_key_constraints` or `enable_foreign_key_constraints` on the session and finish with `close().await?`. SQLite operations that rebuild a table continue to use the connection’s Schema Builder, which manages its own session and transaction.

## Events {#events}

Register listeners in a service provider. The batch events are `MigrationsStarted` and `MigrationsEnded`. Individual execution raises `MigrationStarted` and `MigrationEnded`. An opt-out raises `MigrationSkipped`, and an empty run or rollback raises `NoPendingMigrations`. Pretend mode raises batch events and skips individual execution events.

`MigrationEnded` runs after the migration method succeeds and before its ledger changes. Subscribe to `contracts::database::events::MigrationEvent::NAME` to receive the migration lifecycle events. `SchemaDumped`, `MigrationsPruned`, and `SchemaLoaded` have separate subscriptions. `DatabaseRefreshed` runs after `migrate:fresh` or `migrate:refresh` has applied migrations and before optional seeding.

```rust
use std::sync::Arc;

use openrust::{
    contracts::events::Event as EventPayload,
    database::events::MigrationEnded,
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    support::facades::Event,
};

fn record_migration<'a>(
    event: &'a dyn EventPayload,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        if let Some(ended) = event.downcast_ref::<MigrationEnded>() {
            println!("{:?} {}", ended.method, ended.name);
        }
        Ok(None)
    })
}

fn register_migration_listener() -> EventResult<()> {
    Event::listen(MigrationEnded::NAME, Arc::new(record_migration), Delivery::Sync)
}
```
