# Search

## Full-text search {#full-text-search}

Use a MySQL-compatible connection and create a full-text index with the same column list as the query. The query builder binds the search text as a parameter. `whereFullText` uses natural language mode. Choose `Boolean` or `QueryExpansion` with `whereFullTextWith`.

The example uses an existing database connection named `db`. It creates a table, applies its full-text index, and executes a paginated search.

```rust
use openrust::database::query::FullTextOptions;

db.getSchemaBuilder().create("articles", |table| {
    table.id();
    table.string("title", 150);
    table.text("body");
    table.fullText(&["title", "body"], None);
}).await?;

let results = db.table("articles")
    .whereFullTextWith(&["title", "body"], "+openrust -draft", FullTextOptions::Boolean)
    .orderBy("id", "asc")
    .paginate(10)
    .await?;
```

The same predicate is available on Eloquent queries and nested condition groups. Changes to records update the native index when committed. SQLite returns a query error for this predicate.

### PostgreSQL {#postgresql-full-text}

Set the connection driver to `pgsql`. Blueprint creates a GIN index with the same text-vector expression used by the query. English is the default configuration. This example uses `db` as the PostgreSQL connection.

```rust
db.getSchemaBuilder().create("articles", |table| {
    table.id();
    table.string("title", 150);
    table.text("body");
    table.string("slug", 80).default("");
    table.fullText(&["title", "body"], None).language("english");
}).await?;

let page = db.table("articles")
    .whereFullText(&["title", "body"], "running")
    .orderBy("id", "asc")
    .paginate(10)
    .await?;
```

Use `FullTextOptions::Postgres` with `whereFullTextWith` for `Plain`, `Phrase`, `WebSearch`, or `Raw` query parsing. Set `vector: true` to search existing `tsvector` columns. Use the same language configuration on the index and query. On PostgreSQL, `whereFullText` filters rows and leaves sorting to the query.

## Database model search {#database-engine}

Implement `Searchable` on an Eloquent model to search its own table. Declare indexed columns with `FullText`, prefix matches with `Prefix`, and substring matches with `Contains`. All search alternatives are grouped, so model scopes and extra filters apply to every match. Add the `slug` column to the indexed model in the previous example.

```rust
use openrust::database::eloquent::{SearchColumn, Searchable};

impl Searchable for Article {
    fn searchableColumns() -> &'static [SearchColumn] {
        &[
            SearchColumn::FullText("title"),
            SearchColumn::FullText("body"),
            SearchColumn::Prefix("slug"),
        ]
    }
}

let page = Article::searchOn(&db, "openrust")
    .orderBy("id", "asc")
    .paginate(10)
    .await?;
```

Within the application, use `Article::search(term)?` to resolve the model’s connection. Use `withTrashed` or `onlyTrashed` to change soft-delete filtering. A blank term keeps the model’s regular filters. The database maintains the index as part of the write transaction.

PostgreSQL model searches with full-text columns use `ts_rank` followed by descending primary key when no explicit sort is supplied. Both the rank and the tie-breaker apply before pagination. Override the default full-text settings on a model with `fullTextOptions()`.

### Index visibility after a transaction {#index-transactions}

PostgreSQL updates the table and GIN index in the same transaction. A search inside the write transaction sees its own update or delete. Other sessions see the previous committed rows until the transaction commits. A rollback preserves the previous search results. Use the connection's transaction attempts setting to retry serialization failures and deadlocks.

## Search capabilities {#search-capabilities}

The built-in database search supports MySQL-compatible and PostgreSQL full-text predicates and Eloquent model search. Vector indexes, embedding providers, reranking, and standalone Scout engines require their own drivers.
