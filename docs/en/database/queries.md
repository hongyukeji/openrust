# Query Builder

## Running Database Queries

Start with `Db::table("users")?` in an application lifecycle or `connection.table("users")` on a named connection. Use `transaction.table("users")` inside a transaction to keep the query on its physical write connection. The builder's terminal methods are asynchronous and return a `Result`.

`get` returns a vector of rows. `first` and `find` return an optional row. `value` returns a single value, with `Value::Null` for a missing row, and `pluck` returns a vector of values. Use `exists` or `doesnt_exist` to test for rows. `use_write_pdo` sends a query to the write connection.

### Aggregates

Use `count("*")`, `min("column")`, `max("column")`, `sum("column")`, or `avg("column")`. A count is an unsigned integer. Other aggregates preserve the driver's value type. MySQL exact decimals are returned as text. An empty sum returns zero.

## Streaming Results {#streaming-results}

Call `cursor()` on a query to stream rows. The first poll starts the query. `try_next` reads and decodes one row, and errors are reported through the stream. Connection and query cursors use the configured read route or writer. Dropping a nontransactional cursor releases its lease.

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{Connection, DatabaseResult, Row};

async fn first_streamed_row(db: &Connection) -> DatabaseResult<Option<Row>> {
    let mut rows = db.table("reports").order_by("id", "asc").cursor();
    rows.try_next().await
}
```

Inside a transaction, exhaust the cursor before issuing another statement or committing. Dropping it while the statement is in flight interrupts the transaction and dropping the transaction rolls back its work.

### Chunking Results {#chunking-results}

Call `chunk(count, callback)` on an ordered query. The async callback receives a nonempty batch and a page number starting at 1. Return `Ok(false)` to stop or propagate an error with `Err`. An existing offset skips rows at the start and a limit caps the total. These pages use offsets, so changing which rows match while iterating can skip or repeat rows.

Use `chunk_by_id(count, callback, column, alias)` or `chunk_by_id_desc` to fetch each next batch by comparing a unique, non-null key. Set `column` to `None` for `id`, and `alias` to the name used in the SQL projection if it differs from the column. The first batch honors the offset and the limit caps all batches. Keep the key stable while processing. The callback can stop before the key is checked. Wrap a union in `from_sub` to apply the key filter to all arms.

```rust
use openrust::database::{Connection, DatabaseResult};

async fn process_by_id(db: &Connection) -> DatabaseResult<bool> {
    db.table("reports")
        .select(["id as batch_id"])
        .chunk_by_id_desc(200, |rows, page| async move {
            println!("{}: {}", page, rows.len());
            Ok(true)
        }, None, Some("batch_id"))
        .await
}
```

Use `each(callback, count)` to process one row at a time from offset batches. The index starts at zero in every batch. `each_by_id(callback, count, column, alias)` uses ascending keyset batches and passes the index across the entire scan. Return `Ok(false)` to stop immediately or `Err` to propagate a failure.

Use `lazy(count)`, `lazy_by_id(count, column, alias)` or `lazy_by_id_desc` to consume rows from successive batches as an async stream. Query validation and database errors surface when polling. Dropping the stream stops future fetches. A full keyset batch needs a projected boundary key. A short final batch can finish without it. `chunk_map(callback, count)` maps the rows asynchronously and returns one reindexed collection of results.

## Select Statements

`select` sets the projection and `add_select` extends it. Columns accept an `as` alias. Use `distinct()` to remove duplicate rows. The dialect quotes identifiers and applies the connection's table prefix. Pass a trusted expression explicitly with `select_raw` or `Db::raw`. Bind values separately and select identifiers from application-defined names.

### Index hints {#index-hints}

The last index hint on a query takes effect. MySQL and MariaDB support `use_index`, `force_index`, and `ignore_index`, with one name or a comma-separated list. SQL Server applies `force_index` to a single index and combines it with any row-lock hints on the same table. SQLite applies `force_index` to a single index other than `PRIMARY`. SQL Server and SQLite ignore the other two methods. PostgreSQL ignores all three. Use index names defined by the application.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn indexed_items(connection: &Connection, score: i64) -> DatabaseResult<Vec<Row>> {
    connection
        .table("items")
        .force_index("idx_score")
        .r#where("score", "=", score)
        .order_by("id", "asc")
        .get()
        .await
}
```

## Joins

Use `join`, `left_join`, `right_join`, or `cross_join`. For an ON clause with several conditions, pass a closure to `join_with` or `left_join_with`. `on` compares columns and `r#where` binds a value. Use `join_sub` to join a derived query.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn items_in_region(connection: &Connection, region: &str) -> DatabaseResult<Vec<Row>> {
    let items = connection.table("items as i");
    let groups = items
        .subquery("groups as g")
        .select(["g.id"])
        .r#where("g.region", "=", region);

    items
        .join_sub(&groups, "regional", |join| join.on("i.group_id", "=", "regional.id"))?
        .select(["i.code", "i.stock"])
        .order_by("i.code", "asc")
        .get()
        .await
}
```

### Cross and straight joins {#cross-and-straight-joins}

Use `cross_join_sub` to pair each outer row with every row of a derived query. On MySQL and MariaDB, `straight_join` keeps the left source before the right source in the join order. Use `straight_join_where` to bind a value in the join condition, `straight_join_with` for a closure, or `straight_join_sub` for a derived source.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn label_paid_orders(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    let orders = connection.table("orders as o");
    let labels = orders.subquery("labels").r#where("active", "=", 1);

    orders
        .straight_join("payments as p", "o.id", "=", "p.order_id")
        .cross_join_sub(&labels, "label")?
        .r#where("p.state", "=", "paid")
        .select(["o.id", "label.code"])
        .order_by("label.code", "asc")
        .get()
        .await
}
```

### Lateral Joins {#lateral-joins}

Use `join_lateral` or `left_join_lateral` on PostgreSQL, MySQL 8.0.14+, and SQL Server. Put the correlation in the subquery with `where_column`. The left join keeps outer rows with no match and returns null for their right-side columns. PostgreSQL and MySQL use `JOIN LATERAL`, and SQL Server uses `CROSS APPLY` or `OUTER APPLY`.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn latest_posts(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    let users = connection.table("users as u");
    let posts = users
        .subquery("posts as p")
        .select(["p.id as post_id"])
        .where_column("p.user_id", "=", "u.id")
        .r#where("p.published", "=", 1)
        .order_by_desc("p.id")
        .limit(1);

    users
        .left_join_lateral(&posts, "latest")?
        .select(["u.id", "latest.post_id"])
        .order_by("u.id", "asc")
        .get()
        .await
}
```

## Unions

Call `union(&other)?` to remove duplicate rows, or `union_all(&other)?` to keep them. An order or limit applied before the union belongs to the first query. Apply an order or limit after the union to affect the combined result.

## Basic Where Clauses

Use `r#where(column, operator, value)` for `where`, since `where` is reserved in Rust. `or_where` combines predicates with OR. Group them with `where_nested` or negate the group with `where_not`. Use `where_null`, `where_not_null`, `where_in`, `where_not_in`, `where_between`, or `where_column` for those comparisons. An empty IN matches no rows. An empty NOT IN matches all rows.

`where_like` accepts a case-sensitivity flag. `where_date` and `where_time` use `jiff::civil::Date` and `jiff::civil::Time`. Use `to_sql()` and `get_bindings()` to inspect the SQL and parameters in execution order.

```rust
use openrust::database::{Connection, DatabaseResult, Row, Value};

pub async fn active_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["id", "name"])
        .r#where("active", "=", true)
        .where_nested(|query| {
            query
                .where_null("archived_at")
                .or_where("name", "=", "Admin")
        })
        .order_by("id", "asc")
        .for_page(1, 20)
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

Use `where_exists`, `where_not_exists`, `where_in_sub` or `where_sub` to embed a query. `select_sub` adds a scalar projection and `from_sub` selects from a derived table. Inside a transaction, build the embedded query with `subquery` on the outer builder. The outer query executes the whole statement on its transaction connection.

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
        .where_in_sub("id", &active)?
        .order_by("id", "asc")
        .pluck("id")
        .await
}
```

MySQL runs the entire statement with the outer connection’s permissions. Grant that connection access to the referenced tables.

### Vector Similarity Clauses {#vector-similarity-clauses}

Vector queries use cosine distance on PostgreSQL with the `pgvector` extension and on MariaDB 11.7 or later. See [vector columns](./migrations.md#vector-columns) for enabling the PostgreSQL extension and declaring the column and index.

`where_vector_similar_to` takes a numeric embedding, a minimum similarity from 0 to 1, and a flag for sorting the closest rows first. Pass `false` to provide your own sort. A score of 1 matches an identical direction.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn related_documents(
    connection: &Connection,
    embedding: &[f64],
) -> DatabaseResult<Vec<Row>> {
    connection
        .table("documents")
        .select(["id"])
        .select_vector_distance("embedding", embedding, Some("distance"))?
        .where_vector_similar_to("embedding", embedding, 0.7, true)?
        .limit(10)
        .get()
        .await
}
```

Use `select_vector_distance`, `where_vector_distance_less_than`, `or_where_vector_distance_less_than`, and `order_by_vector_distance` independently when you need finer control. The distance filter includes the maximum distance. When projecting a PostgreSQL vector column, the result contains the vector as text. MariaDB returns the vector as bytes.

Eloquent builders expose the same vector methods. Use the distance predicates inside `where_nested` to group them with other conditions. Eloquent applies global scopes to the full group, including every `or_where_vector_distance_less_than` branch.

```rust
use openrust::database::eloquent::Model;

let mut query = Document::query_on(&connection).select(["id"]);
query.where_nested(|group| {
    group
        .where_vector_distance_less_than("embedding", &primary, 0.3)
        .or_where_vector_distance_less_than("embedding", &alternate, 0.2)
});
let documents = query.order_by_vector_distance("embedding", &primary)?.get().await?;
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
    .select_vector_distance_text("embedding", "Rust web frameworks", Some("distance"))
    .await?
    .where_vector_similar_to_text("embedding", "Rust web frameworks", 0.7, true)
    .await?
    .limit(10)
    .get()
    .await?;
```

## JSON Where Clauses

Use `where_json_contains` and `where_json_doesnt_contain` to test JSON containment, `where_json_contains_key` and `where_json_doesnt_contain_key` to check path existence, and `where_json_length` to compare the length. Separate path segments with `->` and append indexes such as `groups[0]`. Each method has an `or_where` variant.

```rust
use openrust::database::{Connection, DatabaseResult, Row};
use serde_json::json;

pub async fn matching_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .where_json_contains("options->languages", json!("en"))
        .where_json_contains_key("options->timezone")
        .where_json_length("options->languages", ">", 1)
        .get()
        .await
}
```

MySQL and PostgreSQL accept scalar, array, and object candidates. SQLite containment accepts scalar candidates. A key whose value is JSON null still exists. PostgreSQL uses native JSONB containment for both `json` and `jsonb` columns. Its length predicate uses `jsonb_array_length` on the selected array. Key checks support object keys and array indexes, including negative indexes such as `options->languages[-1]`. Missing keys and SQL NULL documents return false for key existence. PostgreSQL escapes path segments as SQL literals and binds the candidate and object key separately.

SQL Server uses `OPENJSON` for scalar containment, key existence, and length comparisons. A length comparison counts array items or object members, and an absent path or SQL NULL document produces a count of zero. Member names are case-sensitive, and array positions can be checked with paths such as `options->languages[0]`. These queries require database compatibility level 130 or higher.

### JSON Overlap {#json-overlap}

`where_json_overlaps` checks whether the selected JSON value shares an element or key-value pair with the candidate. MySQL uses `JSON_OVERLAPS`. `where_json_doesnt_overlap` negates the predicate, and both have `or_where` variants. For a missing path or SQL NULL document, MySQL returns SQL NULL, which a WHERE clause filters out even when the predicate is negated.

```rust
use openrust::database::{Connection, DatabaseResult, Value};
use serde_json::json;

async fn eligible_user_ids(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection
        .table("users")
        .where_json_overlaps("options->languages", json!(["en", "fr"]))
        .order_by("id", "asc")
        .pluck("id")
        .await
}
```

Use a JSON value or an `Expression` containing trusted SQL with separate bindings. SQLite returns an unsupported operation error for these predicates.

## Ordering, Grouping, Limit and Offset

Use `order_by("id", "asc")` or `order_by_desc("id")` to sort. `reorder()` clears the current sort. Use `group_by` and `having` to filter groups. `limit` and `offset` select a range of rows. `for_page(page, per_page)` takes a one-based page number. SQLite and MySQL require a limit when executing an offset. Include a deterministic sort when selecting pages.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn departments(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["department"])
        .select_raw("count(*) as total", Vec::new())
        .group_by(&["department"])
        .having("total", ">=", 2)
        .order_by_desc("total")
        .get()
        .await
}
```

### Value ranges and explicit ordering {#value-ranges-and-ordering}

Use `where_value_between(value, [lower, upper])` to test a bound value against two columns in each row. The bounds are inclusive. The negated form is `where_value_not_between`, and both forms have an OR variant.

`in_order_of` puts listed values first in the supplied order. Rows with unlisted values, including nulls, share the last rank. Add a second sort for ties. An empty list leaves the existing sort intact. Use `Expression` for trusted SQL operands with separate bindings.

```rust
use openrust::database::{Connection, DatabaseResult, Value};

pub async fn ordered_matches(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection.table("order_ranges")
        .where_value_between(5, ["lower_bound", "upper_bound"])
        .in_order_of("status", vec!["active".into(), "pending".into()])
        .order_by("id", "asc")
        .pluck("id")
        .await
}
```

### Limit rows within each group {#group-limit}

Call `group_limit(count, column)` to limit the rows for each value of the partition column. `order_by` selects the rows retained within each group. Use a unique tie-breaker when the order must be deterministic. `offset` skips rows within each group, and a group limit of zero returns no rows.

The builder ranks rows with `row_number()` and hides the rank column from returned rows. Wrap a union in a derived table before applying a group limit.

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn second_per_category(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection.table("group_items")
        .r#where("active", "=", true)
        .order_by_desc("score")
        .order_by("id", "asc")
        .group_limit(1, "category")
        .offset(1)
        .get()
        .await
}
```

## Insert Statements

`insert` accepts one or more `Record` values. Each record in a batch has the same columns. `insert_or_ignore` returns the driver's affected-row count and uses its native ignore behavior. `insert_using` takes a column list and a select query. `insert_get_id` returns the key from the same execution result.

### Generated IDs and inserted rows {#insert-results}

Use `insert_get_id_for(record, "item_key")` to return a named key column on PostgreSQL. Other drivers keep their native generated ID behavior. PostgreSQL also uses `ON CONFLICT DO NOTHING` for `insert_or_ignore` and `insert_or_ignore_using`. SQL Server reports that these ignore operations are unsupported.

On PostgreSQL and SQLite, `insert_or_ignore_returning` returns only newly inserted rows. Supply the columns to return and optionally the unique columns whose conflict should be ignored. A conflict on another constraint still raises an error.

```rust
use openrust::database::{Connection, DatabaseResult, Row, query::Record};

pub async fn register_item(connection: &Connection, code: &str) -> DatabaseResult<Vec<Row>> {
    let item = Record::from([("code".into(), code.into())]);

    connection
        .table("items")
        .insert_or_ignore_returning(&[item], &["item_key", "code"], Some(&["code"]))
        .await
}
```

### Upserts

Use `upsert` with a batch, its conflict columns and the columns to update. SQLite uses the supplied conflict columns. MySQL detects conflicts using the table's unique keys.

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn save_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let mut transaction = connection.begin_transaction().await?;
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

Use `upsert_with` when an update on conflict copies some values from the attempted insert and supplies other values directly. `UpsertUpdate::FromInsert` copies the named insert column, `UpsertUpdate::Value` binds a fixed value, and `UpsertUpdate::Expression` accepts trusted SQL with separate bindings. A row inserted without a conflict uses the insert values and table defaults.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    query::{Record, UpsertUpdate},
};

pub async fn import_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    connection
        .table("items")
        .upsert_with(
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

`update` returns the affected row count. `update_or_insert` performs an existence check followed by a write. Use `upsert` when the insert and conflict handling must run in one statement. `increment` and `decrement` perform arithmetic in SQL and can set extra columns.

PostgreSQL applies joins and limits on updates or deletes by selecting the target rows in a subquery using `ctid`.

### Affected Rows on MySQL and MariaDB {#mysql-affected-rows}

MySQL and MariaDB count rows actually changed by an update. An update that sets a matching row to its current values returns `0`. An upsert reports `1` for an insert, `2` for a changed duplicate row, and `0` for an unchanged duplicate row. Set the connection option `"found_rows": true` to use matched-row counting instead.

### Update expressions and scalar subqueries {#update-expressions}

Use `update_with` to mix bound values, trusted SQL `Expression` values, and `UpdateValue::Subquery` assignments. Build a correlated subquery with `subquery` on the target builder. The database evaluates it for each matching row and determines the result of an empty or multi-row subquery. For JSON paths, MySQL, MariaDB, and PostgreSQL also accept expressions. SQLite merge patches accept literal values.

```rust
use std::collections::BTreeMap;

use openrust::database::{
    Connection, DatabaseResult,
    query::{Expression, UpdateValue},
};

pub async fn refresh_credits(connection: &Connection, id: i64) -> DatabaseResult<u64> {
    let accounts = connection.table("accounts as a");
    let credits = accounts
        .subquery("entries as e")
        .select_raw("sum(amount)", vec![])
        .where_column("e.account_id", "=", "a.id")
        .r#where("e.state", "=", "posted");

    accounts
        .r#where("a.id", "=", id)
        .update_with(BTreeMap::from([
            ("credits".into(), UpdateValue::Subquery(&credits)),
            (
                "note".into(),
                Expression::new("coalesce(?, 'pending')")
                    .with_bindings(vec!["refreshed".into()])
                    .into(),
            ),
        ]))
        .await
}
```

### PostgreSQL updates from joined sources {#postgresql-update-from}

Use `update_from` on PostgreSQL to update a target using joined sources. PostgreSQL lists the sources in `FROM` and applies their join conditions after the query predicates in `WHERE`. As in the framework, this form ignores ordering, limit, and offset. Use a `Record` for bound values or `ValueOperand` to mix bound values with trusted SQL expressions.

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn verify_paid_orders(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("orders as o")
        .join("payments as p", "o.id", "=", "p.order_id")
        .r#where("p.state", "=", "paid")
        .update_from(Record::from([("status".into(), "verified".into())]))
        .await
}
```

### SQL Server joined and limited writes {#sql-server-bounded-writes}

Update and delete operations with joins target the table or its alias using `FROM`. For writes without joins, a positive limit uses `TOP`. With an order, SQL Server selects the target rows using an ordered `TOP` subquery. As in the framework, joined writes and writes with a positive offset apply no limit. A zero limit also omits `TOP`.

```rust
use std::collections::BTreeMap;
use openrust::database::{Connection, DatabaseResult};

pub async fn activate_latest(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("event_archive")
        .r#where("score", "=", 0)
        .order_by("id", "desc")
        .limit(1)
        .update(BTreeMap::from([("score".into(), 1.into())]))
        .await
}
```

### Updating JSON Columns {#updating-json-columns}

Pass a key such as `options->preferences->enabled` to `update`. MySQL applies `JSON_SET` and supports array indexes. SQLite applies a merge patch, where a null member removes that key.

PostgreSQL uses `jsonb_set` on both `json` and `jsonb` columns. Each path segment and the new JSON value are bound separately. Use arrows for object keys and brackets for array indexes, including negative indexes. A dot within a path segment is part of the key. Multiple paths on the same column are applied in record key order within one assignment. JSON null is stored as a member value. Missing intermediate objects are left alone, and a SQL NULL document remains SQL NULL.

On SQL Server, passing a JSON path to `update` returns an unsupported-operation error before executing SQL. You can still replace the entire document by passing the column name and a `Value::Json` value.

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

### SQL Server table truncation {#sql-server-truncate}

SQL Server executes native `TRUNCATE TABLE`. It resets the identity seed and rejects truncation of a table referenced by a foreign key from another table. When run on a transaction handle, rollback restores the data and identity state.

## Pessimistic Locking

Use `lock_for_update` or `shared_lock` inside a transaction. `lock(true)` and `lock(false)` select the same modes. A locking query uses the write connection. SQLite applies its normal transaction locking and omits row-lock clauses.

### Native locking clauses {#native-locking-clauses}

Pass an application-defined SQL clause to `lock` for options such as `FOR UPDATE SKIP LOCKED` on MySQL and PostgreSQL or `WITH (UPDLOCK, ROWLOCK, READPAST)` on SQL Server. On SQL Server, a custom lock combined with `force_index` must use a `WITH (...)` list. Never put request input into the clause.

```rust
use openrust::database::{DatabaseResult, Row, Transaction};

pub async fn next_unlocked(
    transaction: &mut Transaction,
    lock_clause: &'static str,
) -> DatabaseResult<Option<Row>> {
    transaction
        .table("job_slots")
        .order_by("id", "asc")
        .lock(lock_clause)
        .first()
        .await
}
```

## Execution callbacks and raw SQL

`before_query` returns the modified builder and clears callbacks after successful application, for reads and writes. Failures retain callbacks for retry. `after_query` receives and returns `Vec<Row>`; an empty vector means an empty result. `to_raw_sql` reuses driver binding interpolation for debugging only; execution remains parameterized. Because `to_sql` and `to_raw_sql` apply callbacks, they require `&mut self`. Embedded subqueries also require `&mut Builder` to consume callbacks once. Update consumers to declare `let mut query` and pass `&mut query`.

```rust
let mut query = connection.table("users")
    .before_query(|query| Ok(query.r#where("active", "=", true)));
let sql = query.to_raw_sql()?;
let rows = query.after_query(|rows| Ok(rows)).get().await?;
```

## Binding Categories {#binding-categories}

`get_raw_bindings()` returns the ordered categories `select`, `from`, `join`, `where`, `group_by`, `having`, `order`, `union`, and `unionOrder`. `set_bindings(values, kind)` replaces a category; `add_binding(value_or_values, kind)` appends to it. Use `None` for the default `where` category or `Some("join")` for an explicit category. Unknown categories return an error. These methods consume and return the builder through `Result`.

```rust
let query = connection.table("users")
    .where_raw("id > ? and id < ?", Vec::new())
    .add_binding([1_i64, 10], None)?
    .set_bindings(vec![2.into(), 9.into()], None)?
    .r#where("active", "=", true);
let bindings = query.get_raw_bindings()?;
assert_eq!(bindings["where"], vec![2.into(), 9.into(), true.into()]);
```

Later conditions append their values after a manual replacement. `select` clears projection bindings, and `reorder` clears both regular and union order bindings. `merge_bindings(&other)` appends each category without copying SQL; ensure the SQL contains the matching placeholders. The typed values remain bound parameters, including values that contain SQL text. `get_bindings()` reports the parameters used by the compiled query; grouped limits move ordering parameters into the window projection and add bound numeric limits. `get_raw_bindings()` reports the original logical categories.

## Additional Query Conditions {#additional-query-conditions}

`from_raw(sql, bindings)` sets a trusted SQL source. `join_where`, `left_join_where` and `right_join_where` compare a join column with a bound value. Raw SQL must come from application code; pass user values through bindings.

`where_binary`, `where_not_binary` and their `or` variants use MySQL binary comparisons. Other drivers return an unsupported-operation error. The four `where_integer_in_raw` / `where_integer_not_in_raw` variants accept `Vec<i64>`; empty positive sets match nothing and empty negative sets match everything. `where_row_values` and `or_where_row_values` return `Result`, reject unequal column/value counts and invalid operators, and bind every value.

`having_null` / `having_not_null` and their `or` variants accept a column slice. `having_between` / `having_not_between` and their `or` variants accept two bounds. `having_nested` and `or_having_nested` receive a query builder and group its HAVING conditions; an empty callback adds no condition.

`in_random_order(None)` uses the driver's random function; `Some(seed)` supplies a MySQL seed. `reorder()` also clears union ordering, and `reorder_desc(column)` starts a new descending order. `for_page_before_id(per_page, last_id, column)` and `for_page_after_id` replace ordering for that column while keeping other sorts. Pass `None` for `last_id` to select non-null IDs without an ID boundary.

`raw_value(expression, bindings)` appends a raw projection and returns the first column of the first row, or `Value::Null` for no row. Existing projections retain priority. PostgreSQL can infer an untyped parameter projection as text, so `select ?` may return `Value::Text`; use an explicit SQL cast when the result needs a particular type.

`exists_or` and `doesnt_exist_or` accept a lazy async callback returning `DatabaseResult<T>`. They return `query::ExistsOr::Satisfied` when the condition holds, or `Callback(T)` after running the callback. Query errors skip the callback; callback errors propagate.
