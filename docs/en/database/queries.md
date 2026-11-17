# Query Builder

## Running Database Queries

Start with `DB::table("users")?` in an application lifecycle or `connection.table("users")` on a named connection. Use `transaction.table("users")` inside a transaction to keep the query on its physical write connection. The builder's terminal methods are asynchronous and return a `Result`.

`get` returns a vector of rows. `first` and `find` return an optional row. `value` returns a single value, with `Value::Null` for a missing row, and `pluck` returns a vector of values. Use `exists` or `doesntExist` to test for rows. `useWritePdo` sends a query to the write connection.

### Aggregates

Use `count("*")`, `min("column")`, `max("column")`, `sum("column")`, or `avg("column")`. A count is an unsigned integer. Other aggregates preserve the driver's value type. MySQL exact decimals are returned as text. An empty sum returns zero.

## Streaming Results {#streaming-results}

Call `cursor()` on a query to stream rows. The first poll starts the query. `try_next` reads and decodes one row, and errors are reported through the stream. Connection and query cursors use the configured read route or writer. Dropping a nontransactional cursor releases its lease.

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{Connection, DatabaseResult, Row};

async fn first_streamed_row(db: &Connection) -> DatabaseResult<Option<Row>> {
    let mut rows = db.table("reports").orderBy("id", "asc").cursor();
    rows.try_next().await
}
```

Inside a transaction, exhaust the cursor before issuing another statement or committing. Dropping it while the statement is in flight interrupts the transaction and dropping the transaction rolls back its work.

### Chunking Results {#chunking-results}

Call `chunk(count, callback)` on an ordered query. The async callback receives a nonempty batch and a page number starting at 1. Return `Ok(false)` to stop or propagate an error with `Err`. An existing offset skips rows at the start and a limit caps the total. These pages use offsets, so changing which rows match while iterating can skip or repeat rows.

Use `chunkById(count, callback, column, alias)` or `chunkByIdDesc` to fetch each next batch by comparing a unique, non-null key. Set `column` to `None` for `id`, and `alias` to the name used in the SQL projection if it differs from the column. The first batch honors the offset and the limit caps all batches. Keep the key stable while processing. The callback can stop before the key is checked. Wrap a union in `fromSub` to apply the key filter to all arms.

```rust
use openrust::database::{Connection, DatabaseResult};

async fn process_by_id(db: &Connection) -> DatabaseResult<bool> {
    db.table("reports")
        .select(["id as batch_id"])
        .chunkByIdDesc(200, |rows, page| async move {
            println!("{}: {}", page, rows.len());
            Ok(true)
        }, None, Some("batch_id"))
        .await
}
```

Use `each(callback, count)` to process one row at a time from offset batches. The index starts at zero in every batch. `eachById(callback, count, column, alias)` uses ascending keyset batches and passes the index across the entire scan. Return `Ok(false)` to stop immediately or `Err` to propagate a failure.

Use `lazy(count)`, `lazyById(count, column, alias)` or `lazyByIdDesc` to consume rows from successive batches as an async stream. Query validation and database errors surface when polling. Dropping the stream stops future fetches. A full keyset batch needs a projected boundary key. A short final batch can finish without it. `chunkMap(callback, count)` maps the rows asynchronously and returns one reindexed collection of results.

## Select Statements

`select` sets the projection and `addSelect` extends it. Columns accept an `as` alias. Use `distinct()` to remove duplicate rows. The dialect quotes identifiers and applies the connection's table prefix. Pass a trusted expression explicitly with `selectRaw` or `DB::raw`. Bind values separately and select identifiers from application-defined names.

## Joins

Use `join`, `leftJoin`, `rightJoin`, or `crossJoin`. For an ON clause with several conditions, pass a closure to `joinWith` or `leftJoinWith`. `on` compares columns and `r#where` binds a value. Use `joinSub` to join a derived query. MySQL supports correlated `joinLateral` and `leftJoinLateral`.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn items_in_region(connection: &Connection, region: &str) -> DatabaseResult<Vec<Row>> {
    let items = connection.table("items as i");
    let groups = items
        .subquery("groups as g")
        .select(["g.id"])
        .r#where("g.region", "=", region);

    items
        .joinSub(&groups, "regional", |join| join.on("i.group_id", "=", "regional.id"))?
        .select(["i.code", "i.stock"])
        .orderBy("i.code", "asc")
        .get()
        .await
}
```

## Unions

Call `union(&other)?` to remove duplicate rows, or `unionAll(&other)?` to keep them. An order or limit applied before the union belongs to the first query. Apply an order or limit after the union to affect the combined result.

## Basic Where Clauses

Use `r#where(column, operator, value)` for Laravel's `where`, since `where` is reserved in Rust. `orWhere` combines predicates with OR. Group them with `whereNested` or negate the group with `whereNot`. Use `whereNull`, `whereNotNull`, `whereIn`, `whereNotIn`, `whereBetween`, or `whereColumn` for those comparisons. An empty IN matches no rows. An empty NOT IN matches all rows.

`whereLike` accepts a case-sensitivity flag. `whereDate` and `whereTime` use `chrono::NaiveDate` and `chrono::NaiveTime`. Use `toSql()` and `getBindings()` to inspect the SQL and parameters in execution order.

```rust
use openrust::database::{Connection, DatabaseResult, Row, Value};

pub async fn active_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["id", "name"])
        .r#where("active", "=", true)
        .whereNested(|query| {
            query
                .whereNull("archived_at")
                .orWhere("name", "=", "Admin")
        })
        .orderBy("id", "asc")
        .forPage(1, 20)
        .get()
        .await
}

pub async fn active_count(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("users")
        .r#where("active", "=", Value::Bool(true))
        .count("*")
        .await
}
```

## Advanced Where Clauses

Use `whereExists`, `whereNotExists`, `whereInSub` or `whereSub` to embed a query. `selectSub` adds a scalar projection and `fromSub` selects from a derived table. Inside a transaction, build the embedded query with `subquery` on the outer builder. The outer query executes the whole statement on its transaction connection.

### MySQL Subqueries Across Schemas {#mysql-cross-schema-subqueries}

Builders for two MySQL schemas can be combined when their configured server endpoints match. The builder prefixes the inner query’s unqualified primary table with its schema. Tables that already include a schema retain it. Qualify any additional joined tables or prebuilt derived sources explicitly.

```rust
use openrust::database::{Connection, DatabaseResult, Value};

async fn active_customers(
    primary: &Connection,
    reporting: &Connection,
) -> DatabaseResult<Vec<Value>> {
    let active = reporting
        .table("customer_activity")
        .select(["customer_id"])
        .r#where("active", "=", 1);
    primary
        .table("customers")
        .whereInSub("id", &active)?
        .orderBy("id", "asc")
        .pluck("id")
        .await
}
```

MySQL runs the entire statement with the outer connection’s permissions. Grant that connection access to the referenced tables.

### Vector Similarity Clauses {#vector-similarity-clauses}

Vector queries use cosine distance on PostgreSQL with the `pgvector` extension and on MariaDB 11.7 or later. See [vector columns](./migrations.md#vector-columns) for enabling the PostgreSQL extension and declaring the column and index.

`whereVectorSimilarTo` takes a numeric embedding, a minimum similarity from 0 to 1, and a flag for sorting the closest rows first. Pass `false` to provide your own sort. A score of 1 matches an identical direction.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn related_documents(
    connection: &Connection,
    embedding: &[f64],
) -> DatabaseResult<Vec<Row>> {
    connection
        .table("documents")
        .select(["id"])
        .selectVectorDistance("embedding", embedding, Some("distance"))?
        .whereVectorSimilarTo("embedding", embedding, 0.7, true)?
        .limit(10)
        .get()
        .await
}
```

Use `selectVectorDistance`, `whereVectorDistanceLessThan`, `orWhereVectorDistanceLessThan`, and `orderByVectorDistance` independently when you need finer control. The distance filter includes the maximum distance. When projecting a PostgreSQL vector column, the result contains the vector as text. MariaDB returns the vector as bytes.

Eloquent builders expose the same vector methods. Use the distance predicates inside `whereNested` to group them with other conditions. Eloquent applies global scopes to the full group, including every `orWhereVectorDistanceLessThan` branch.

```rust
use openrust::database::eloquent::Model;

let mut query = Document::queryOn(&connection).select(["id"]);
query.whereNested(|group| {
    group
        .whereVectorDistanceLessThan("embedding", &primary, 0.3)
        .orWhereVectorDistanceLessThan("embedding", &alternate, 0.2)
});
let documents = query.orderByVectorDistance("embedding", &primary)?.get().await?;
```

#### Text Embeddings {#text-embeddings}

Register an `Embeddings` service in the application container. The built-in `HttpEmbeddingProvider` accepts the complete URL of an OpenAI-compatible embeddings endpoint, the model, its token, and optional dimensions. Read these settings from trusted application configuration. For another provider, implement `EmbeddingProvider` and register it with `Embeddings::new`.

```rust
use std::sync::Arc;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::query::embeddings::{Embeddings, HttpEmbeddingProvider},
};

let provider = HttpEmbeddingProvider::new(&endpoint, model, token, Some(1536))?;
app.container().instance(Arc::new(Embeddings::new(provider)))?;
```

In Query Builder and Eloquent, the vector methods with a `Text` suffix resolve the embedding asynchronously before adding the SQL clause. The numeric methods remain synchronous. Query text is sent to the configured provider on a cache miss. Successful embeddings are cached for 30 days in the application’s default cache, keyed by provider identity and input. The built-in identity includes the endpoint, model, credentials, and dimensions.

```rust
let documents = connection
    .table("documents")
    .select(["id"])
    .selectVectorDistanceText("embedding", "Rust web frameworks", Some("distance"))
    .await?
    .whereVectorSimilarToText("embedding", "Rust web frameworks", 0.7, true)
    .await?
    .limit(10)
    .get()
    .await?;
```

## JSON Where Clauses

Use `whereJsonContains` and `whereJsonDoesntContain` to test JSON containment, `whereJsonContainsKey` and `whereJsonDoesntContainKey` to check path existence, and `whereJsonLength` to compare the length. Separate path segments with `->` and append indexes such as `groups[0]`. Each method has an `orWhere` variant.

```rust
use openrust::database::{Connection, DatabaseResult, Row};
use serde_json::json;

pub async fn matching_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .whereJsonContains("options->languages", json!("en"))
        .whereJsonContainsKey("options->timezone")
        .whereJsonLength("options->languages", ">", 1)
        .get()
        .await
}
```

MySQL and PostgreSQL accept scalar, array, and object candidates. SQLite containment accepts scalar candidates. A key whose value is JSON null still exists. PostgreSQL uses native JSONB containment for both `json` and `jsonb` columns. Its length predicate uses `jsonb_array_length` on the selected array. Key checks support object keys and array indexes, including negative indexes such as `options->languages[-1]`. Missing keys and SQL NULL documents return false for key existence. PostgreSQL escapes path segments as SQL literals and binds the candidate and object key separately.

### JSON Overlap {#json-overlap}

`whereJsonOverlaps` checks whether the selected JSON value shares an element or key-value pair with the candidate. MySQL uses `JSON_OVERLAPS`. `whereJsonDoesntOverlap` negates the predicate, and both have `orWhere` variants. For a missing path or SQL NULL document, MySQL returns SQL NULL, which a WHERE clause filters out even when the predicate is negated.

```rust
use openrust::database::{Connection, DatabaseResult, Value};
use serde_json::json;

async fn eligible_user_ids(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection
        .table("users")
        .whereJsonOverlaps("options->languages", json!(["en", "fr"]))
        .orderBy("id", "asc")
        .pluck("id")
        .await
}
```

Use a JSON value or an `Expression` containing trusted SQL with separate bindings. SQLite returns an unsupported operation error for these predicates.

## Ordering, Grouping, Limit and Offset

Use `orderBy("id", "asc")` or `orderByDesc("id")` to sort. `reorder()` clears the current sort. Use `groupBy` and `having` to filter groups. `limit` and `offset` select a range of rows. `forPage(page, per_page)` takes a one-based page number. SQLite and MySQL require a limit when executing an offset. Include a deterministic sort when selecting pages.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn departments(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["department"])
        .selectRaw("count(*) as total", Vec::new())
        .groupBy(&["department"])
        .having("total", ">=", 2)
        .orderByDesc("total")
        .get()
        .await
}
```

### Value ranges and explicit ordering {#value-ranges-and-ordering}

Use `whereValueBetween(value, [lower, upper])` to test a bound value against two columns in each row. The bounds are inclusive. The negated form is `whereValueNotBetween`, and both forms have an OR variant.

`inOrderOf` puts listed values first in the supplied order. Rows with unlisted values, including nulls, share the last rank. Add a second sort for ties. An empty list leaves the existing sort intact. Use `Expression` for trusted SQL operands with separate bindings.

```rust
use openrust::database::{Connection, DatabaseResult, Value};

pub async fn ordered_matches(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection.table("order_ranges")
        .whereValueBetween(5, ["lower_bound", "upper_bound"])
        .inOrderOf("status", vec!["active".into(), "pending".into()])
        .orderBy("id", "asc")
        .pluck("id")
        .await
}
```

### Limit rows within each group {#group-limit}

Call `groupLimit(count, column)` to limit the rows for each value of the partition column. `orderBy` selects the rows retained within each group. Use a unique tie-breaker when the order must be deterministic. `offset` skips rows within each group, and a group limit of zero returns no rows.

The builder ranks rows with `row_number()` and hides the rank column from returned rows. Wrap a union in a derived table before applying a group limit.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn second_per_category(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection.table("group_items")
        .r#where("active", "=", true)
        .orderByDesc("score")
        .orderBy("id", "asc")
        .groupLimit(1, "category")
        .offset(1)
        .get()
        .await
}
```

## Insert Statements

`insert` accepts one or more `Record` values. Each record in a batch has the same columns. `insertOrIgnore` returns the driver's affected-row count and uses its native ignore behavior. `insertUsing` takes a column list and a select query. `insertGetId` returns the key from the same execution result.

### Upserts

Use `upsert` with a batch, its conflict columns and the columns to update. SQLite uses the supplied conflict columns. MySQL detects conflicts using the table's unique keys.

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn save_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let mut transaction = connection.beginTransaction().await?;
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    let affected = transaction
        .table("items")
        .upsert(&[row], &["code"], Some(&["stock"]))
        .await?;
    transaction.commit().await?;
    Ok(affected)
}
```

#### Assignments on Conflict {#upsert-conflict-assignments}

Use `upsertWith` when an update on conflict copies some values from the attempted insert and supplies other values directly. `UpsertUpdate::FromInsert` copies the named insert column, `UpsertUpdate::Value` binds a fixed value, and `UpsertUpdate::Expression` accepts trusted SQL with separate bindings. A row inserted without a conflict uses the insert values and table defaults.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    query::{Record, UpsertUpdate},
};

pub async fn import_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    connection
        .table("items")
        .upsertWith(
            &[row],
            &["code"],
            &[
                UpsertUpdate::FromInsert("stock"),
                UpsertUpdate::Value("source", "inventory import".into()),
            ],
        )
        .await
}
```

## Update Statements

`update` returns the affected row count. `updateOrInsert` performs an existence check followed by a write. Use `upsert` when the insert and conflict handling must run in one statement. `increment` and `decrement` perform arithmetic in SQL and can set extra columns.

PostgreSQL applies joins and limits on updates or deletes by selecting the target rows in a subquery using `ctid`.

### Updating JSON Columns {#updating-json-columns}

Pass a key such as `options->preferences->enabled` to `update`. MySQL applies `JSON_SET` and supports array indexes. SQLite applies a merge patch, where a null member removes that key.

PostgreSQL uses `jsonb_set` on both `json` and `jsonb` columns. Each path segment and the new JSON value are bound separately. Use arrows for object keys and brackets for array indexes, including negative indexes. A dot within a path segment is part of the key. Multiple paths on the same column are applied in record key order within one assignment. JSON null is stored as a member value. Missing intermediate objects are left alone, and a SQL NULL document remains SQL NULL.

```rust
use std::collections::BTreeMap;
use openrust::database::{Connection, DatabaseResult};

pub async fn enable_notifications(connection: &Connection, id: i64) -> DatabaseResult<u64> {
    connection
        .table("users")
        .r#where("id", "=", id)
        .update(BTreeMap::from([
            ("options->preferences->enabled".into(), true.into()),
            ("options->preferences->channel".into(), "email".into()),
        ]))
        .await
}
```

## Delete Statements

Apply conditions and call `delete()` to get the affected row count. Without a filter the deletion applies to the entire table. Use `truncate()` to clear a table and reset its generated IDs. Run it outside a MySQL transaction because MySQL implicitly commits this statement.

### PostgreSQL table truncation {#postgresql-truncate}

PostgreSQL uses `TRUNCATE … RESTART IDENTITY CASCADE` by default. It clears the target table and the tables that reference it through foreign keys, and resets their owned sequences. Set `cascade_on_truncate` to `false` on a PostgreSQL connection to use PostgreSQL’s default refusal when other tables reference the target. When run on a transaction handle, the data and sequence resets roll back with that transaction.

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn reset_events(connection: &Connection) -> DatabaseResult<()> {
    connection.table("event_archive").truncate().await
}
```

## Pessimistic Locking

Use `lockForUpdate` or `sharedLock` inside a transaction. MySQL holds the row locks until commit or rollback. SQLite uses its normal transaction locking and has no equivalent row lock clause.
