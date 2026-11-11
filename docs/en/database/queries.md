# Query Builder

## Running Database Queries

Start with `DB::table("users")?` in an application lifecycle or `connection.table("users")` on a named connection. Use `transaction.table("users")` inside a transaction to keep the query on its physical write connection. The builder's terminal methods are asynchronous and return a `Result`.

`get` returns a vector of rows. `first` and `find` return an optional row. `value` returns a single value, with `Value::Null` for a missing row, and `pluck` returns a vector of values. Use `exists` or `doesntExist` to test for rows. `useWritePdo` sends a query to the write connection.

### Aggregates

Use `count("*")`, `min("column")`, `max("column")`, `sum("column")`, or `avg("column")`. A count is an unsigned integer. Other aggregates preserve the driver's value type. MySQL exact decimals are returned as text. An empty sum returns zero.

## Select Statements

`select` sets the projection and `addSelect` extends it. Columns accept an `as` alias. Use `distinct()` to remove duplicate rows. The dialect quotes identifiers and applies the connection's table prefix. Pass a trusted expression explicitly with `selectRaw` or `DB::raw`. Bind values separately and select identifiers from application-defined names.

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
