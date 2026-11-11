# Query Builder

## Running Database Queries

Start with `DB::table("users")?` in an application lifecycle or `connection.table("users")` on a named connection. Use `transaction.table("users")` inside a transaction to keep the query on its physical write connection. The builder's terminal methods are asynchronous and return a `Result`.

`get` returns a vector of rows. `first` and `find` return an optional row. `value` returns a single value, with `Value::Null` for a missing row, and `pluck` returns a vector of values. Use `exists` or `doesntExist` to test for rows. `useWritePdo` sends a query to the write connection.

### Aggregates

Use `count("*")`, `min("column")`, `max("column")`, `sum("column")`, or `avg("column")`. A count is an unsigned integer. Other aggregates preserve the driver's value type. MySQL exact decimals are returned as text. An empty sum returns zero.

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

Use `whereExists`, `whereNotExists`, `whereInSub` or `whereSub` with a query on the same named connection. `selectSub` adds a scalar projection and `fromSub` selects from a derived table. Inside a transaction, build the embedded query with `subquery` on the outer builder. The outer query executes the whole statement on its transaction connection.

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

## Update Statements

`update` returns the affected row count. `updateOrInsert` performs an existence check followed by a write. Use `upsert` when the insert and conflict handling must run in one statement. `increment` and `decrement` perform arithmetic in SQL and can set extra columns.

### Updating JSON Columns

Pass a key such as `options->preferences->enabled` to `update`. MySQL applies `JSON_SET` and supports array indexes. SQLite applies a merge patch, where a null member removes that key.

## Delete Statements

Apply conditions and call `delete()` to get the affected row count. Without a filter the deletion applies to the entire table. Use `truncate()` to clear a table and reset its generated IDs. Run it outside a MySQL transaction because MySQL implicitly commits this statement.

## Pessimistic Locking

Use `lockForUpdate` or `sharedLock` inside a transaction. MySQL holds the row locks until commit or rollback. SQLite uses its normal transaction locking and has no equivalent row lock clause.
