# Migrations

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

## Indexes

Use `index`, `unique` or `primary` with a column slice. Pass an explicit name or `None` for the conventional name. Use `getIndexes`, `hasIndex` and `renameIndex` to inspect or rename an index. MySQL supports full-text indexes on compatible columns.

### Foreign Key Constraints

Use `foreignId("author_id").constrained()` for a reference to `authors.id`. Use `constrainedOn` to name the referenced table and column explicitly. Apply column modifiers before defining the foreign key, and referential actions such as `cascadeOnDelete()` afterward. Use `getForeignKeys` to inspect constraints. SQLite exposes the constrained columns in its catalog, and `dropForeignColumns` can remove a table-level constraint by those columns.
