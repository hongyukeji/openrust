# Migrations

## Generating Migrations

Use `make:migration` to create a timestamped Rust source file in `database/migrations`. Use `--create=flights` or `--table=flights` to select the stub explicitly.

```sh
./artisan make:migration create_flights_table
```

During a build, OpenRust discovers Rust migrations in `database/migrations/`. Use the `YYYY_MM_DD_HHMMSS_name.rs` filename format and expose a `Definition` implementing `Migration`. This applies to both handwritten migrations and files created by `make:migration`. Rebuild the application to include them. The generated list is passed to `withMigrations` in `bootstrap/app.rs`. The filename still determines the ledger name, and `--path` selects from the compiled migrations. For generation into a custom directory, register those entries with the application separately.

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

## Rolling Back Migrations

`migrate:rollback` rolls back the latest batch in reverse order. Use `--step` to select a number of migrations, or `--batch` to select a specific batch.

```sh
./artisan migrate:rollback --pretend
./artisan migrate:rollback --step=1
```

Use `migrate:reset` to roll back all migrations and `migrate:refresh` to roll them back and apply them again. When the ledger exists, `migrate:fresh` drops all tables on the selected database, including tables with other prefixes, and reruns migrations. Add `--drop-views` to drop views as well.

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

During `Schema::create`, use `engine`, `charset`, and `collation` to set the table options. MySQL uses a blueprint setting first and then the connection default for that option. `comment` can be used when creating or altering a table. Read the applied engine, collation and comment with `getTables()`.

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_customers() -> DatabaseResult<()> {
    Schema::create("customers", |table| {
        table.engine("InnoDB");
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
