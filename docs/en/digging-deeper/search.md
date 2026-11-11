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

## Search capabilities {#search-capabilities}

This database engine supports MySQL-compatible full-text predicates and Eloquent model search. PostgreSQL full-text queries, vector indexes, embedding providers, reranking, and standalone Scout engines require their own drivers.
