# Migrations

## Generating Migrations

Use `make:migration` to create a timestamped Rust source file in `database/migrations`. Use `--create=flights` or `--table=flights` to select the stub explicitly.

```sh
./artisan make:migration create_flights_table
```

Register the generated module and its `MigrationEntry` in `database/migrations/mod.rs` and rebuild. The registered source path supplies the ledger name and can be selected using `--path`.

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

## Columns

A blueprint closure receives a mutable `Blueprint`. Column modifiers follow the column definition, such as `table.string("isbn", 20).nullable()`. Use `id()` for a generated primary key and `timestamps()` for nullable creation and update timestamps. Length, precision and scale are explicit Rust arguments.

Use `change` to specify a replacement column definition, including the modifiers to keep. `renameColumn` and `dropColumn` use native driver operations. `getColumns`, `getColumnListing` and `getColumnType` read column metadata from the catalog.

SQLite rebuilds the table for column and foreign key changes that require it. Run a rebuild through the connection's schema builder outside an active transaction. The rebuild owns its transaction and restores the connection's foreign key settings. MySQL DDL that implicitly commits also runs through the connection's schema builder.

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
