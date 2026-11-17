# Migrations

## Generating Migrations

Use `make:migration` to create a timestamped Rust source file in `database/migrations`. Use `--create=flights` or `--table=flights` to select the stub explicitly.

```sh
./artisan make:migration create_flights_table
```

During a build, OpenRust discovers Rust migrations in `database/migrations/`. Use the `YYYY_MM_DD_HHMMSS_name.rs` filename format and expose a `Definition` implementing `Migration`. This applies to both handwritten migrations and files created by `make:migration`. Rebuild the application to include them. The generated list is passed to `withMigrations` in `bootstrap/app.rs`. The filename still determines the ledger name, and `--path` selects from the compiled migrations. For generation into a custom directory, register those entries with the application separately.

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

Implement `up` to apply a migration and `down` to reverse it. Use the supplied `MigrationContext` for schema changes and SQL so the migrator can collect the statements in pretend mode. Implement `getConnection` to select a different connection for the migration, or `shouldRun` to conditionally skip it.

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
            connection.getSchemaBuilder().create("flights", |table| {
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
            connection.getSchemaBuilder().dropIfExists("flights").await
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

### Migration Transactions {#migration-transactions}

On PostgreSQL and SQL Server, each migration runs its `up` or `down` method in a transaction by default. Use the supplied `MigrationContext` to run Schema changes and SQL on that transaction. Migration listeners can use the transaction supplied by their event context. If the migration or a synchronous lifecycle listener fails before commit, its changes roll back and the ledger keeps its prior state.

Override `withinTransaction()` in the migration’s `impl Migration` block for operations that require autocommit, such as [PostgreSQL online indexes](#postgresql-online-indexes). An opted-out migration can leave completed changes in place after a later failure. `--pretend` displays SQL without starting the migration transaction.

```rust
fn withinTransaction(&self) -> bool {
    false
}
```

### Checking for Pending Migrations {#checking-pending-migrations}

Use `--pending` to list only pending migrations. Use `--pending=17` to make the command return 17 when that list is nonempty. An empty list returns 0. The explicit exit code must be between 0 and 255. A missing migration ledger returns 1. Use `--path` and `--database` to scope the check.

```sh
./artisan migrate:status --pending=17
```

Before the first pending migration, a migration whose `shouldRun` condition is false appears as `Skipped`. Subsequent unrecorded migrations appear as `Pending` because earlier migrations may change the database state used by their conditions.

### Reporting Migration Failures as Warnings {#graceful-migrations}

Use `--graceful` to report an execution error as a warning and return exit code 0. This covers migration and seeding errors. The failed migration remains pending, and any earlier successful migrations remain recorded. Production confirmation still applies.

```sh
./artisan migrate --graceful
```

### Excluding Pending Migrations {#excluding-pending-migrations}

Call `withoutMigrations` on the application’s migrator from a service provider to exclude named migrations from apply and pretend operations. Use the migration name or its `.rs` path. A later call replaces the list, and an empty slice clears it. Status continues to report the ledger and migration conditions, and rollback uses the recorded batch history.

```rust
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::migrations::Migrator,
    foundation::Application,
    support::ProviderResult,
};

fn configure_migrations(app: &Application) -> ProviderResult {
    let migrator = app.container().get::<Migrator>("migrator")?;
    migrator.withoutMigrations(&["database/migrations/0001_create_first.rs"]);
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

Use `Schema::create("books", |table| { … }).await?` to create a table on the default connection. Use `Schema::connection("reporting")?` to select a named connection. An explicit connection exposes the same API through `getSchemaBuilder()`.

Use `table` to alter a table, `rename` to rename it and `dropIfExists` for conditional removal. Use `hasTable` and `getTables` to inspect the database. Schema operations use the write connection.

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_catalog(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();

    schema.create("authors", |table| {
        table.id();
        table.string("name", 120);
    }).await?;

    schema.create("books", |table| {
        table.id();
        table.foreignId("author_id").constrained().cascadeOnDelete();
        table.string("title", 200);
        table.unique(&["author_id", "title"], None);
        table.timestamps();
    }).await?;

    schema.table("books", |table| {
        table.string("isbn", 20).nullable();
        table.index(&["isbn"], Some("books_isbn_index"));
    }).await?;

    assert!(schema.hasColumns("books", &["title", "isbn"]).await?);
    assert!(schema.hasIndex("books", "books_isbn_index").await?);
    Ok(())
}
```

### MySQL Table Options {#mysql-table-options}

During `Schema::create`, use `engine`, `charset`, and `collation` to set the table options. MySQL uses a blueprint setting first and then the connection default for that option. `comment` can be used when creating or altering a table. Read the applied engine, collation and comment with `getTables()`. Use `innoDb()` as shorthand for `engine("InnoDB")`.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_customers() -> DatabaseResult<()> {
    Schema::create("customers", |table| {
        table.innoDb();
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

### Temporary Tables {#temporary-tables}

Acquire a schema session before using `Blueprint::temporary()`. Create the table through the session’s Schema Builder and run SQL through the same session. `close().await?` drops the managed temporary tables before returning the connection to the pool. If the session is dropped, cleanup retains the physical connection until it can be safely released.

```rust
use openrust::database::{DatabaseResult, Value, schema::Schema};

async fn staged_code() -> DatabaseResult<Value> {
    let mut session = Schema::session().await?;
    session.getSchemaBuilder().create("import_codes", |table| {
        table.temporary();
        table.string("code", 20);
    }).await?;
    session.statement("INSERT INTO import_codes (code) VALUES (?)", &["pending".into()]).await?;
    let rows = session.select("SELECT code FROM import_codes", &[]).await?;
    session.close().await?;
    Ok(rows[0].get_at(0).cloned().unwrap_or(Value::Null))
}
```

SQLite exposes its temporary table metadata on the same session. In MySQL, `hasTable()` checks the session’s managed temporary tables. Use a session SQL statement such as `SHOW COLUMNS` to inspect them.

## Columns

A blueprint closure receives a mutable `Blueprint`. Column modifiers follow the column definition, such as `table.string("isbn", 20).nullable()`. Use `id()` for a generated primary key and `timestamps()` for nullable creation and update timestamps. Length, precision and scale are explicit Rust arguments.

Use `change` to specify a replacement column definition, including the modifiers to keep. `renameColumn` and `dropColumn` use native driver operations. `getColumns`, `getColumnListing` and `getColumnType` read column metadata from the catalog.

SQLite rebuilds the table for column and foreign key changes that require it. Run a rebuild through the connection's schema builder outside an active transaction. The rebuild owns its transaction and restores the connection's foreign key settings. MySQL DDL that implicitly commits also runs through the connection's schema builder.

### Additional Column Types {#additional-column-types}

Use `tinyText`, `year`, `ipAddress`, `macAddress` and `r#enum` to declare the corresponding columns. A year column with `useCurrent()` defaults to the database’s current year. MySQL stores native ENUM and SET values. SQLite enforces enum values with a CHECK constraint and returns an unsupported schema error for SET.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_events() -> DatabaseResult<()> {
    Schema::create("incoming_events", |table| {
        table.smallIncrements("id");
        table.r#enum("status", &["pending", "processed"]).default("pending");
        table.year("batch_year").useCurrent();
        table.ipAddress("source_ip").nullable();
        table.binaryWith("digest", 32, true).nullable();
        table.timestampsTz();
    })
    .await
}
```

`binary()` declares a blob. `binaryWith(name, length, fixed)` uses MySQL BINARY or VARBINARY and SQLite BLOB. MySQL uses its native widths for `tinyIncrements`, `smallIncrements`, `mediumIncrements` and the matching unsigned integer helpers. SQLite uses INTEGER. On both drivers, the `Tz` helpers use the corresponding native temporal types.

### ULID Column Length {#ulid-column-length}

Use `ulidWith(name, length)` and `foreignUlidWith(name, length)` to declare a different character width for imported identifiers. `ulid` and `foreignUlid` keep the standard width of 26. These methods set the column width. A standard ULID still contains 26 characters.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_import_tables() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.ulidWith("id", 32).primary();
    }).await?;
    Schema::create("import_entries", |table| {
        table.id();
        table.foreignUlidWith("import_id", 32)
            .constrainedOn("imports", "id", None)
            .cascadeOnDelete();
    }).await
}
```

### Inspecting SQLite Columns {#inspecting-sqlite-columns}

For a SQLite column declared as `VARCHAR(32)`, `getColumnType(..., false)` returns `varchar` and `getColumnType(..., true)` returns `varchar(32)`. Both return an empty string for a column declared without a type.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_title() -> DatabaseResult<(String, String)> {
    Ok((
        Schema::getColumnType("books", "title", false).await?,
        Schema::getColumnType("books", "title", true).await?,
    ))
}
```

Column metadata reports `auto_increment` when an integer primary key aliases the rowid and SQLite can generate its value. The `nullable` flag reflects whether the column can store a null. For example, an ordinary SQLite composite primary key can permit nulls unless constrained by `NOT NULL`, `STRICT`, or `WITHOUT ROWID`.

### Editing a Blueprint {#editing-a-blueprint}

Use `idWith("import_id")` to name an auto-incrementing primary key. It corresponds to Laravel’s `id("import_id")`. Use `id()` for the default name.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_imports() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.idWith("import_id");
        table.string("source", 32);
        table.text("temporary_notes");
        table.removeColumn("temporary_notes");
    })
    .await
}
```

Use `removeColumn()` inside a blueprint callback to discard a pending column addition or change. It removes modifiers declared on that column. Explicit indexes and constraints remain in the blueprint. Use `dropColumn()` to drop an existing database column.

### Inspecting a Blueprint {#inspecting-a-blueprint}

In a schema callback, `getTable()` returns the supplied table reference before the framework applies the connection prefix. `creating()` distinguishes `Schema::create` from `Schema::table` and conditional alterations.

```rust
use openrust::database::schema::Blueprint;

fn prepare_status(table: &mut Blueprint) {
    if table.creating() {
        table.id();
    }

    if !table
        .getColumns()
        .any(|column| column.getName() == "status")
    {
        table.string("status", 24).default("pending");
    }
}
```

Use `getColumns()`, `getAddedColumns()`, and `getChangedColumns()` to read pending definitions in declaration order. These iterators borrow the blueprint. Use the Schema facade’s catalog methods to inspect columns already in the database.

### Resolving a Blueprint {#resolving-a-blueprint}

Set a resolver on a schema builder to add shared definitions before the operation’s callback runs. The resolver receives a mutable blueprint with the table reference, connection prefix and creation state already set.

```rust
use openrust::database::schema::Builder;

fn use_import_defaults(schema: &mut Builder<'_>) {
    schema.blueprintResolver(|table| {
        if table.creating() {
            table.id();
            table.string("source", 32);
        }
    });
}
```

A schema builder keeps its own resolver. Register one on the builder used by the migration, transaction or session. Migration previews compile the same definitions into SQL.

### Reading the Connection Table Prefix {#reading-the-connection-table-prefix}

Use `DB::getTablePrefix()` for the default connection or `DB::connection(Some("archive"))?.getTablePrefix()` for a named connection. Inside a blueprint callback, `getPrefix()` reports the prefix of that blueprint’s connection.

```rust
use openrust::{database::DatabaseResult, support::facades::DB};

fn migration_prefixes() -> DatabaseResult<(String, String)> {
    let default = DB::getTablePrefix()?;
    let archive = DB::connection(Some("archive"))?;
    Ok((default, archive.getTablePrefix().to_owned()))
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

Use `virtualAs` to compute a column when it is read or `storedAs` to store the computed result. Both take a trusted SQL expression. `virtualAsJson` and `storedAsJson` use an arrow-separated JSON path. A generated column must have a declared type, and the database computes its value from the other columns.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_invoice_lines() -> DatabaseResult<()> {
    Schema::create("invoice_lines", |table| {
        table.id();
        table.integer("quantity");
        table.integer("unit_cents");
        table.integer("total_cents").storedAs("quantity * unit_cents");
        table.json("metadata").nullable();
        table.string("currency", 3).virtualAsJson("metadata->currency").nullable();
    })
    .await
}
```

`getColumns()` exposes the generation mode and expression. MySQL and SQLite support both modes when creating tables. SQLite’s native ALTER can add a virtual column. Add a stored column when creating a table, or use a migration that explicitly rebuilds the table.

#### MySQL Generated Columns and Current Defaults {#mysql-generated-columns-and-current-defaults}

MySQL generated columns allow NULL by default. Use `nullableWith(false)` to require a value, or `nullable()` to allow NULL explicitly. Regular columns are required by default.

On MySQL, `useCurrent()` takes precedence over an explicit default on DATE, YEAR, DATETIME, and TIMESTAMP, regardless of modifier order. The database supplies the current date, year, or timestamp, using column precision for timestamps. Other column types keep their explicit default.

```rust
use openrust::database::schema::Blueprint;

fn define_import_metrics(table: &mut Blueprint) {
    table.id();
    table.integer("quantity").nullable();
    table.integer("doubled").virtualAs("quantity * 2");
    table.integer("total")
        .storedAs("COALESCE(quantity, 0)")
        .nullableWith(false);
    table.timestamp("recorded_at", 3)
        .useCurrent()
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
        table.geography("location", Some("point"), None).spatialIndex();
        table.geometry("outline", Some("polygon"), Some(4326)).nullable();
    })
    .await
}
```

MySQL enforces the declared subtype and any nonzero SRID. A non-null spatial column can declare an index with `spatialIndex()`. SQLite declares a `geometry` column and supports storing binary values. Spatial functions and spatial indexes require a spatial-capable database.

### Vector Columns {#vector-columns}

On PostgreSQL, enable the installed `pgvector` extension before declaring a vector column. Pass `None` to use the default schema or `Some(schema)` to place the extension explicitly. The extension must be visible on the connection’s search path. MariaDB 11.7 and later provide vector columns natively.

```rust
use openrust::support::facades::Schema;

Schema::ensureVectorExtensionExists(None).await?;
```

```rust
use openrust::support::facades::Schema;

Schema::create("documents", |table| {
    table.id();
    table.vector("embedding", Some(1536)).index();
}).await?;
```

Both drivers accept an explicit dimension. PostgreSQL also accepts `None` for a column without a fixed dimension. Use a fixed dimension for the default cosine index. MariaDB requires an explicit dimension and a non-null indexed column.

Calling `index()` on a vector column creates a cosine index. PostgreSQL uses HNSW with `vector_cosine_ops`. MariaDB uses `VECTOR INDEX` with `M=6` and cosine distance. Use `vectorIndex("embedding", Some("semantic_idx"))` to choose a name. On PostgreSQL, `algorithm("ivfflat")` and `operatorClass("vector_l2_ops")` select another index method and distance operator class. The operator class must match the distance expression used by the query.

Drop a named index with `dropVectorIndex("semantic_idx")` or the conventional index with `dropVectorIndexColumns(&["embedding"])`. See [vector similarity queries](./queries.md#vector-similarity-clauses) for querying the column.

On MariaDB, `operatorClass("M=8 DISTANCE=euclidean")` sets vector index options. `M` accepts 3 through 200, and `DISTANCE` accepts `cosine` or `euclidean`. The Query Builder’s vector search methods use cosine distance.

To build a PostgreSQL vector index while accepting writes, see [PostgreSQL online indexes](#postgresql-online-indexes).

### MySQL Column Modifiers

Use `invisible()` to omit a column from `SELECT *` while keeping it available by name. `useCurrentOnUpdate()` updates a timestamp or datetime when another value in the row changes. Add `useCurrent()` if the insert default should also use the current time. Use `first()` or `after("column")` when adding or changing columns on an existing MySQL table. On table creation, declare columns in the desired order.

```rust
Schema::table("books", |table| {
    table.string("internal_note", 120).nullable().invisible();
    table.timestamp("reviewed_at", 6).useCurrent().useCurrentOnUpdate();
    table.string("edition", 20).nullable().after("title");
}).await?;
```

## Indexes

Use `index`, `unique` or `primary` with a column slice. Pass an explicit name or `None` for the conventional name. Use `getIndexes`, `hasIndex` and `renameIndex` to inspect or rename an index. MySQL supports full-text indexes on compatible columns.

### Foreign Key Constraints

Use `foreignId("author_id").constrained()` for a reference to `authors.id`. Use `constrainedOn` to name the referenced table and column explicitly. Apply column modifiers before defining the foreign key, and referential actions such as `cascadeOnDelete()` afterward. Use `getForeignKeys` to inspect constraints. SQLite exposes the constrained columns in its catalog, and `dropForeignColumns` can remove a table-level constraint by those columns.

#### Default Column Length and Time Precision {#default-column-length-and-time-precision}

`Schema::defaultStringLength` and `Schema::defaultTimePrecision` set defaults for subsequent definitions in the current application. The initial defaults are 255 characters and 0 fractional seconds digits. Pass `None` to an individual column to use the application default, or pass an explicit length or precision.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_entries() -> DatabaseResult<()> {
    Schema::defaultStringLength(120)?;
    Schema::defaultTimePrecision(Some(3))?;

    Schema::create("entries", |table| {
        table.id();
        table.string("label", None);
        table.timestamps();
        table.softDeletesDatetime(Some("removed_at"), 6);
    })
    .await
}
```

`timestamps()` and `timestampsTz()` use the configured precision. Use `timestampsWith` or `timestampsTzWith` for an explicit precision. `datetimes` creates nullable DATETIME columns for `created_at` and `updated_at`. `softDeletesDatetime` creates the nullable DATETIME soft-delete column. SQLite uses its native column declarations.

#### MySQL Column Lengths and Update Expressions {#mysql-column-lengths-and-update-expressions}

On MySQL, use `char(name, 0)` for an empty fixed-width character column. `CHAR` allows up to 255 characters. A zero length in `binaryWith` behaves like `binary` and creates a BLOB. Positive lengths create BINARY or VARBINARY depending on the `fixed` argument.

Use `onUpdate` with a trusted SQL expression for a TIMESTAMP or DATETIME column. Match the expression’s fractional precision to the column. When both update modifiers are specified, `useCurrentOnUpdate` uses the column’s precision and takes precedence.

```rust
use openrust::database::{query::Expression, schema::Blueprint};

fn define_import_status(table: &mut Blueprint) {
    table.id();
    table.char("flag", 0).nullable();
    table.binaryWith("payload", 0, false).nullable();
    table.dateTime("changed_at", 3)
        .useCurrent()
        .onUpdate(Expression::new("NOW(3)"));
}
```

#### Floating-Point Precision and Raw Columns {#float-precision-and-raw-columns}

`float()` uses Laravel’s default precision of 53 bits. Use `floatWith(name, precision)` to set it explicitly. On MySQL, precision 1–24 selects FLOAT and 25–53 selects DOUBLE. Use precision 0 for a native FLOAT declaration without a precision argument. SQLite uses its native floating-point behavior regardless of the requested precision.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_measurements() -> DatabaseResult<()> {
    Schema::create("measurements", |table| {
        table.id();
        table.float("value");
        table.floatWith("device_value", 24);
        table.rawColumn("sample_count", "smallint").default(0);
    })
    .await
}
```

Pass a trusted SQL column definition to `rawColumn(name, definition)` in a migration. Schema quotes the column name and appends the usual modifiers. The database interprets the supplied definition.

#### Column Groups and Drop Aliases {#column-groups-and-drop-aliases}

`morphs()` and `nullableMorphs()` use the application’s configured morph key type. Use `numericMorphs`, `uuidMorphs`, `ulidMorphs`, or their nullable variants to select the ID type explicitly. For an explicit index name or MySQL column position, pass those options to the typed functions, `morphsWith`, or `nullableMorphs`. Use `Schema::dropColumns(table, columns)` to remove columns without a Blueprint callback.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_attachments() -> DatabaseResult<()> {
    Schema::create("attachments", |table| {
        table.id();
        table.nullableMorphs("attachable", Some("attachments_attachable"), None);
        table.rememberToken();
        table.nullableTimestamps();
    })
    .await
}

async fn remove_attachment_metadata() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.dropMorphs("attachable", Some("attachments_attachable"));
        table.dropRememberToken();
        table.dropTimestamps();
    })
    .await
}

async fn remove_legacy_columns() -> DatabaseResult<()> {
    Schema::dropColumns("attachments", &["legacy_name", "legacy_rank"]).await
}
```

`dropMorphs` drops the composite index before removing both columns. Pass the customized name if the index was named explicitly. `dropConstrainedForeignId` drops the foreign key and then the column. `dropSoftDeletes(None)` and `dropSoftDeletesTz(None)` remove the default `deleted_at` column, while `Some(name)` selects a different column.

To add several columns in order after an existing MySQL column, use `table.after("title", |table| { … })` inside `Schema::table`. SQLite reports the positioning modifier as unsupported.

#### Default Expressions {#default-expressions}

Pass a trusted SQL expression to `default()` using `DB::raw()` or `Expression::new()`. The database evaluates the expression when an insert omits the column. Passing an ordinary string stores it as a literal default.

```rust
use openrust::{database::DatabaseResult, support::facades::{DB, Schema}};

async fn create_flights() -> DatabaseResult<()> {
    Schema::create("flights", |table| {
        table.id();
        table.json("movies").default(DB::raw("(JSON_ARRAY())"));
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

Use `from()` or `startingValue()` to set the starting value of an auto-increment column on MySQL. MySQL determines the next available value using the existing rows. SQLite uses its native sequence and rejects the MySQL online DDL modifiers.

#### Auto-Incrementing Columns and Explicit Primary Keys {#auto-incrementing-columns-and-explicit-primary-keys}

Use `integerIncrements("id")` as an alias for `increments("id")`. Both create an unsigned integer. The `autoIncrement()` modifier preserves the signedness of the column type. On MySQL, a table-level `primary` declaration supplies the entire primary key. InnoDB requires the auto-increment column to be the leading column in an index.

```rust
use openrust::database::schema::Blueprint;

fn define_import_entries(table: &mut Blueprint) {
    table.integer("entry_id").autoIncrement().startingValue(500);
    table.integer("import_id");
    table.primary(&["entry_id", "import_id"], None);
}
```

#### Conditional Schema Changes {#conditional-schema-changes}

Use `whenTableHasColumn`, `whenTableDoesntHaveColumn`, `whenTableHasIndex`, and `whenTableDoesntHaveIndex` to run a blueprint only when its catalog check matches. The check and subsequent schema statement are separate operations.

```rust
use openrust::{
    database::{DatabaseResult, schema::KeySelector},
    support::facades::Schema,
};

async fn prepare_imports() -> DatabaseResult<()> {
    Schema::whenTableDoesntHaveColumn("imports", "status", |table| {
        table.string("status", 24).default("pending");
    })
    .await?;

    Schema::whenTableDoesntHaveIndex(
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

`hasIndexWith` and both index callbacks accept a `KeySelector` by name or exact column order, with an optional type such as `primary`, `unique`, or `btree`. `hasForeignKey` accepts the same selector on the local columns.

#### Creating and Dropping Databases {#creating-and-dropping-databases}

`createDatabase` creates a database on the selected MySQL server using the connection’s configured character set and collation. It returns an error if the name already exists. `dropDatabaseIfExists` removes the named database and succeeds if it is already absent.

On PostgreSQL, both methods execute database-level DDL outside transactions. Creation uses the connection’s configured character set as the database encoding when supplied. Connect to an existing database before creating another one. To drop a database, connect to a different database on the same server.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_reporting_database() -> DatabaseResult<()> {
    Schema::createDatabase("reporting_staging").await?;
    Ok(())
}

async fn drop_reporting_database() -> DatabaseResult<()> {
    Schema::dropDatabaseIfExists("reporting_staging").await?;
    Ok(())
}
```

On SQLite, supply a file path. Creation fails if the path already exists, preserving the existing database. Close connections to the file before removing it.

#### Dropping All Tables or Views {#dropping-all-tables-or-views}

`dropAllTables()` removes permanent tables from the selected connection’s current schema. `dropAllViews()` removes views from that schema. These methods apply to all objects of the requested kind, even when a table prefix is configured. Use a named connection when targeting a separate database.

PostgreSQL follows the [same schema scope and table exclusions as `db:wipe`](#rebuilding-mysql-with-fresh). Use `dropAllTypes()` to drop its user-defined types and domains.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn clear_reporting_schema() -> DatabaseResult<()> {
    let mut schema = Schema::connection("reporting_staging")?;
    schema.dropAllViews().await?;
    schema.dropAllTables().await
}
```

The table operation leaves views in place. A view that references a removed table may need to be dropped or recreated. MySQL table removal temporarily disables foreign key checks on the write session and restores the previous setting.

#### Inspecting Tables {#inspecting-tables}

`getTables(None)` lists tables across visible user schemas. Pass `Some(name)` to limit the query to one schema or use `getTablesIn(&[...])` for several. Use an explicit schema when checking the result of a schema-scoped wipe.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_tables(schema: &str) -> DatabaseResult<()> {
    let names = Schema::getTableListing(Some(schema)).await?;
    for name in names {
        println!("{name}");
    }
    Ok(())
}
```

`getTableListing` returns qualified names. Use `getTableListingWith(schema, false)` for names without a schema prefix. Table entries contain catalog names and, when supplied by the database, size and storage metadata.

#### Parsing Table References {#parsing-table-references}

`parseSchemaAndTable(reference, fallback)` splits a logical table reference before the connection adds its table prefix. A qualified reference keeps its explicit schema. For a bare table name, use `SchemaFallback::None`, `SchemaFallback::Current`, or `SchemaFallback::Named(name)`. Use `Schema::connection(name)` to select a connection. Table references accept at most two nonempty parts.

```rust
use openrust::{
    database::{DatabaseResult, schema::SchemaFallback},
    support::facades::Schema,
};

async fn list_tables_in_same_schema(reference: &str) -> DatabaseResult<Vec<String>> {
    let (schema, _) = Schema::parseSchemaAndTable(reference, SchemaFallback::Current)?;
    Schema::getTableListing(schema.as_deref()).await
}
```

#### Inspecting a Column {#inspecting-a-column}

`Schema::getColumn` returns the metadata for one column. Column names are compared case-insensitively. Rust returns `None` when the column is absent, and database errors propagate through `DatabaseResult`.

On PostgreSQL, column inspection includes identity and serial flags, primary-key membership, and generated expressions. An unqualified table name uses the server’s current schema. Pass a qualified name to inspect a table in another schema.

```rust
use openrust::{
    database::{DatabaseResult, schema::Column},
    support::facades::Schema,
};

async fn inspect_title() -> DatabaseResult<Option<Column>> {
    Schema::getColumn("posts", "title").await
}
```

#### Inspecting Schemas and Views {#inspecting-schemas-and-views}

`getSchemas()` lists the visible schemas and marks the selected one. SQLite also reports each schema’s file path. `getViews(None)` returns views from all visible user schemas on MySQL or all schemas on the SQLite write session. Use `getViews(Some(name))` for one schema or `getViewsIn(&[...])` for an explicit list. `getCurrentSchemaName()` and `getCurrentSchemaListing()` use the configured write connection. MySQL uses its configured database, and SQLite uses `main`. These two methods read connection settings without issuing SQL.

On PostgreSQL, `getCurrentSchemaListing()` returns the configured `search_path` in order, with `$user` resolved to the selected endpoint’s username. Use a comma-separated string or an array of schema names. `getCurrentSchemaName()` returns the first entry. The database catalog methods query the selected writer, and `getSchemas()` marks the schema currently in use by the server.

`Schema::getTypes(None)` lists PostgreSQL types across user schemas. Pass `Some(schema)` or use `Schema::getTypesIn(&[...])` to limit the scope. Each result provides a qualified name, type, category, and an `implicit` flag for generated arrays and multiranges. Other drivers return an unsupported-schema error.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_status_view() -> DatabaseResult<()> {
    if Schema::hasView("status_report").await? {
        let current = Schema::getCurrentSchemaName()?;
        for view in Schema::getViews(Some(&current)).await? {
            println!("{}", view.schema_qualified_name);
        }
    }
    Ok(())
}
```

`hasView("name")` checks the current schema. Use a qualified name to check another schema. The connection’s table prefix applies to `hasView`. View entries report catalog names and may include a definition.

#### Renaming and Changing a Column {#renaming-and-changing-a-column}

On MySQL, combine `renameTo()` with `change()` to rename a column and set its new definition in one ALTER statement. The definition starts with the current name. Include the modifiers you want to keep.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn revise_import_labels() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.string("old_label", 100).nullable().renameTo("label").change();
    })
    .await
}
```

For a rename alone, use `renameColumn`. On SQLite, use `renameColumn` for the rename and a separate `change()` for the new definition.

#### Changing Columns and Their Indexes {#changing-columns-and-their-indexes}

A plain `change()` preserves indexes and foreign keys. Use `uniqueWith(false)` or `indexWith(false)` with `change()` to remove an index by its conventional name. When renaming the column during the same operation, removal uses its old name and a new index uses the new column.

Use `indexWith("name")`, `uniqueWith("name")`, `fullTextWith("name")`, or `spatialIndexWith("name")` to add a named column index. To remove a custom-named index, use the corresponding table-level drop method. MySQL requires an index that backs a foreign key until you explicitly drop the constraint or provide another suitable index.

```rust
use openrust::database::schema::Blueprint;

fn change_region_code(table: &mut Blueprint) {
    table.string("postal_code", 30)
        .uniqueWith(false)
        .indexWith("region_lookup")
        .renameTo("region_code")
        .change();
}
```

#### Replacing a MySQL Primary Key {#replacing-a-mysql-primary-key}

Call `dropPrimary()` immediately before `primary(...)` in the same MySQL table callback. OpenRust emits one ALTER TABLE statement. Keep an auto-increment column first in a suitable index, such as the new primary key in this example. Add index options such as `inplace()` or `lock("none")` to the replacement declaration when the engine supports them.

```rust
use openrust::database::schema::Blueprint;

fn replace_import_key(table: &mut Blueprint) {
    table.dropPrimary();
    table.primary(&["id", "tenant_id"], None);
}
```

#### Adding an Auto-Increment Column and a Primary Key {#adding-an-auto-increment-column-and-a-primary-key}

On an existing MySQL table, declare the new auto-increment column immediately before its primary key in the same callback. OpenRust emits one ALTER TABLE statement. Keep the auto-increment column first in the primary key when it has no other supporting index. To replace an existing primary key, call `dropPrimary()` immediately before the column declaration.

```rust
use openrust::database::schema::Blueprint;

fn add_import_key(table: &mut Blueprint) {
    table.bigIncrements("id");
    table.primary(&["id", "tenant_id"], None);
}
```

Column and index lock settings must agree when both are specified. MySQL validates the chosen algorithm and lock mode for the combined operation.

#### Dropping Indexes by Column List {#dropping-indexes-by-columns}

Pass the ordered column list to `dropIndexColumns`, `dropUniqueColumns`, `dropFullTextColumns`, or `dropSpatialIndexColumns` to drop an index created with the default name for its kind. For a custom name, use the corresponding method that takes the name.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn remove_import_lookup() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.dropIndexColumns(&["status", "created_at"]);
    })
    .await
}
```

#### Inspecting SQLite Primary Keys {#sqlite-primary-indexes}

On SQLite, `getIndexes()` reports the native name for a primary key with a backing index. A rowid primary key without a backing index appears as `primary`. An ordinary index named `primary` can appear alongside it.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn primary_columns() -> DatabaseResult<Vec<Option<String>>> {
    let indexes = Schema::getIndexes("deliveries").await?;
    Ok(indexes
        .into_iter()
        .find(|index| index.primary)
        .map(|index| index.columns)
        .unwrap_or_default())
}
```

Use the `primary` flag to identify the constraint and read its columns in key order. `hasIndex("deliveries", "primary")` also recognizes a primary key with a native index name.

#### Raw Indexes {#raw-indexes}

`rawIndex(expression, name)` adds a named index using a database-specific SQL key list. MySQL requires an extra pair of parentheses around each functional key part. SQLite accepts `lower(label)` directly. Supply SQL from your migration code. Drop the index with `dropIndex(name)`.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn index_labels() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.rawIndex("(lower(label))", "imports_lower_label");
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
        table.dropColumn(&["legacy_code", "legacy_note"])
            .instant()
            .lock("default");
    })
    .await
}
```

Pass multiple names to `dropColumn` to drop them in one MySQL ALTER statement. Chain `instant()` or `lock()` to request an online DDL option. If the engine rejects the statement, the columns in that statement remain in place. Earlier statements in the same blueprint may already have completed.

#### PostgreSQL Unique Constraints {#postgresql-unique-constraints}

Use `unique()` to create a named PostgreSQL constraint. On PostgreSQL 15 and later, `nullsNotDistinct(true)` treats NULL values as equal. `nullsNotDistinct(false)` explicitly keeps them distinct. Use `deferrable(true).initiallyImmediate(false)` to check uniqueness at commit. With `online()` or an explicit `algorithm("btree")`, the builder creates the supporting index before attaching it to the constraint. `dropUnique()` removes the constraint and its index. It can also remove a standalone unique index created by earlier OpenRust migrations on the same table.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn require_unique_reference() -> DatabaseResult<()> {
    Schema::table("documents", |table| {
        table
            .unique(&["external_id"], Some("documents_external_unique"))
            .nullsNotDistinct(true)
            .deferrable(true)
            .initiallyImmediate(false);
    })
    .await
}
```

#### PostgreSQL Online Indexes {#postgresql-online-indexes}

Call `online()` on an ordinary, unique, full-text, or vector index to use PostgreSQL’s `CREATE INDEX CONCURRENTLY`. The index can wait for existing transactions while other sessions continue writing.

```rust
use openrust::support::facades::Schema;

Schema::table("documents", |table| {
    table.vectorIndex("embedding", Some("documents_vector_search")).online();
}).await?;
```

Inside a migration, override [`withinTransaction()`](#migration-transactions) to return `false`. Use the connection’s Schema builder. A blueprint containing an online index runs its statements individually in autocommit mode. Keep that blueprint separate from changes you want to apply atomically. An active framework-managed transaction rejects the blueprint before executing any of its statements. A failed concurrent build can leave an invalid index, which you can drop before retrying.

#### Foreign IDs from Models {#foreign-ids-from-models}

Use `foreignIdFor::<Model>(None)` to let an Eloquent model supply the reference column name. `constrained()` uses the model’s table and primary key, including custom names. Pass `Some(name)` to use a different column name in the referencing table.

In this example, `PhotoAsset` uses a UUID primary key named `photo_code` in the `photo_assets` table.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn add_photo_reference() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.foreignIdFor::<PhotoAsset>(Some("photo_ref"))
            .nullable()
            .constrained()
            .nullOnDelete();
    })
    .await
}

async fn drop_photo_reference() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.dropConstrainedForeignIdFor::<PhotoAsset>(Some("photo_ref"));
    })
    .await
}
```

Model IDs default to an unsigned big integer. A model that generates UUIDs or ULIDs for its primary key selects the corresponding column type. Use `foreignUuidFor` and `foreignUlidFor` to select a format explicitly, or override `Model::getKeyType` for manually assigned IDs. Use `dropForeignIdFor` for a column that has no constraint.

#### Naming a Foreign Key {#naming-foreign-keys}

Use `constrainedWith(table, column, index_name)` to override individual parts of a foreign key. An omitted table or column keeps the value supplied by the model. Without a model, the referenced column defaults to `id`, and the table name is inferred by pluralizing the part of the local column name before the last `_<referenced column>`. Pass an explicit name to `dropForeign` when removing a named constraint.

```rust
Schema::table("attachments", |table| {
    table.foreignIdFor::<PhotoAsset>(Some("photo_ref"))
        .nullable()
        .constrainedWith(None, None, Some("attachments_photo"))
        .nullOnDelete();
}).await?;
```

#### SQLite Inline Constraints {#sqlite-inline-constraints}

Changing a SQLite column preserves its existing inline primary key, UNIQUE, CHECK and REFERENCES clauses. The rebuild also preserves the auto-increment sequence when the rebuilt table retains its auto-increment key. The column definition passed to `change()` controls the type, nullability and default.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn relax_owner_reference() -> DatabaseResult<()> {
    Schema::table("orders", |table| {
        table.dropForeignColumns(&["owner_id"]);
        table.integer("owner_id").nullable().change();
    })
    .await
}
```

Use `dropForeignColumns` for a foreign key’s local columns, or `dropForeign` for an explicitly named constraint. Both forms can remove inline REFERENCES or table-level foreign keys. Use `dropPrimary` to remove an inline or table-level primary key. Other inline constraints remain in force.

#### PostgreSQL Foreign Keys {#postgresql-foreign-keys}

Use qualified table names to add a foreign key across schemas. By default, PostgreSQL checks a new constraint against existing rows. Call `notValid()` to skip that scan while still checking new rows and changes to the foreign key. Call `deferrable(true).initiallyImmediate(false)` to check at transaction commit. Use `initiallyImmediate(true)` to check at the end of each statement by default. Drop a key with `dropForeign(name)` or `dropForeignColumns(&[...])` for a conventionally named key.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn constrain_books() -> DatabaseResult<()> {
    Schema::table("library.books", |table| {
        table
            .foreign(&["author_id"], Some("books_author_foreign"))
            .references("id")
            .on("authors.records")
            .deferrable(true)
            .initiallyImmediate(false)
            .notValid();
    })
    .await
}
```

#### Inspecting Foreign Keys {#inspecting-foreign-keys}

Use `getForeignKeys` to inspect the local columns, target schema and table, target columns, and update and delete actions. A composite key’s columns are returned in constraint order.

On PostgreSQL, the catalog reports the constraint name and the referenced schema. Local and referenced columns are paired in constraint order, including compound keys that target a different schema.

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn author_schema() -> DatabaseResult<Option<String>> {
    Ok(Schema::getForeignKeys("books")
        .await?
        .into_iter()
        .find(|key| key.columns == ["author_id"])
        .and_then(|key| key.foreign_schema))
}
```

SQLite reports the containing schema, such as `main` or an attached database name. Its catalog leaves the constraint name unset. When the declaration omits the referenced column names, the corresponding entries are `None` and SQLite uses the referenced table’s primary key.

#### Toggling Foreign Key Constraints {#toggling-foreign-key-constraints}

Use `Schema::withoutForeignKeyConstraints` to run work on a single write session with foreign key checks disabled. Use the supplied session for SQL and its `getSchemaBuilder()` for schema operations. The session restores its original setting on exit.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn import_order() -> DatabaseResult<()> {
    Schema::withoutForeignKeyConstraints(|session| {
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

For manual control, acquire a write session with `Schema::session().await?` or the named builder’s `session().await?`, then call `disableForeignKeyConstraints` or `enableForeignKeyConstraints` on the session and finish with `close().await?`. SQLite operations that rebuild a table continue to use the connection’s Schema Builder, which manages its own session and transaction.

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
