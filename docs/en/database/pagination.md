# Database Pagination

## Basic Usage

`paginate(15)` runs a total count and fetches one page. Use `simplePaginate(15)` when only previous and next links are needed. It fetches one extra row to detect a following page. Both work on Query Builder and Eloquent. Eloquent defaults to primary-key order. Set an explicit order on Query Builder for stable pages.

Use `paginateWith` or `simplePaginateWith` with `PageOptions` to change the page name, select columns or supply an explicit page number. HTTP uses the current request’s query string. Outside HTTP, the default page is 1.

## Cursor Pagination

`cursorPaginate(15)` compares the values of ordered columns and fetches one extra row. Pass the returned `next_cursor` or `prev_cursor` in the request. In Query Builder, order by selected, non-null scalar columns with a unique final tie-breaker. An Eloquent query without an explicit order uses the primary key. Grouped queries, unions and raw order expressions use offset pagination.

## JSON and Links

Paginators serialize when their items implement Serde’s `Serialize`. Use `map` to convert rows or Eloquent models into your response items without discarding page metadata. Call `withQueryString` to retain filters or `appends` to add selected parameters. Links use request-relative paths.

```rust
use openrust::database::{DB, DatabaseResult};

async fn article_ids(table: &str) -> DatabaseResult<impl serde::Serialize> {
    let mut page = DB::table(table)?
        .orderBy("id", "asc")
        .paginate(15)
        .await?;
    page.withQueryString();
    Ok(page.map(|row| match row.get("id") {
        Some(openrust::database::Value::Integer(id)) => Some(id.to_string()),
        Some(openrust::database::Value::Unsigned(id)) => Some(id.to_string()),
        _ => None,
    }))
}
```
