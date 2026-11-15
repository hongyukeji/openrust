# Database Pagination

## Basic Usage

`paginate(15)` runs a total count and fetches one page. Use `simplePaginate(15)` when only previous and next links are needed. It fetches one extra row to detect a following page. Both work on Query Builder and Eloquent. Eloquent defaults to primary-key order. Set an explicit order on Query Builder for stable pages.

Use `paginateWith` or `simplePaginateWith` with `PageOptions` to change the page name, select columns or supply an explicit page number. HTTP uses the current request’s query string. Outside HTTP, the default page is 1.

## Cursor Pagination

`cursorPaginate(15)` compares the values of ordered columns and fetches one extra row. Pass the returned `next_cursor` or `prev_cursor` in the request. In Query Builder, order by selected, non-null scalar columns with a unique final tie-breaker. An Eloquent query without an explicit order uses the primary key. Grouped queries, unions and raw order expressions use offset pagination.

## Paginating Related Models {#paginating-related-models}

Call `paginate`, `simplePaginate`, or `cursorPaginate` on a `HasMany` or `MorphMany` descriptor with the parent model and page size. The `With` variants accept the same options as Eloquent queries. The related query retains the parent key, the morph type where applicable, configured filters, and model scopes. A parent without a key yields an empty page.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasMany}},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn related_comments<P: Model, R: Model>(
    article: &P,
    comments: &HasMany<P, R>,
) -> DatabaseResult<LengthAwarePaginator<R>> {
    comments.paginateWith(article, PageOptions {
        perPage: Some(15),
        pageName: "comments",
        ..Default::default()
    }).await
}
```

`HasManyThrough` and `HasOneThrough` also accept these pagination methods. The query applies the intermediate join and excludes soft-deleted intermediate models by default. Call `withTrashedParents()` to include them. The default projection contains only columns from the related model. Qualify custom sort columns when the joined tables share the same column name.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasManyThrough}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn through_page<P: Model, I: Model, R: Model>(
    parent: &P,
    relation: &HasManyThrough<P, I, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation.cursorPaginateWith(parent, CursorOptions {
        perPage: Some(15),
        cursorName: "reactions",
        ..Default::default()
    }).await
}
```

`BelongsToMany` and `MorphToMany` support the same paginator methods. Each model on the current page carries its Pivot, including any custom model or accessor. Cursor pagination selects columns used by `orderByPivot` for cursor generation. Use `withPivot` to expose additional columns on the Pivot. The ordered columns together must uniquely identify each row.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}},
    pagination::CursorPaginator,
};

async fn pivot_page<P: Model, R: Model>(
    parent: &P,
    relation: BelongsToMany<P, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation
        .withPivot(["label"])
        .orderByPivot("weight", "asc")
        .orderByPivot("label_id", "asc")
        .cursorPaginate(parent, 15)
        .await
}
```

## Displaying Pagination Results {#displaying-pagination-results}

Use the paginator’s `links()` to create a Blade view. The default Tailwind view shows the result range and numbered links for `paginate`. Both `simplePaginate` and `cursorPaginate` show previous and next links. Navigation is empty when all results fit on one page. `onEachSide` adjusts the numbered window.

```rust
use openrust::{
    contracts::view::View as _,
    pagination::LengthAwarePaginator,
    view::ViewResult,
};

fn pagination_navigation<T>(page: &LengthAwarePaginator<T>) -> ViewResult<String> {
    page.links()?.render()
}
```

Use `linksWith("articles.navigation", data)` for a custom view, or override the default at `resources/views/vendor/pagination/tailwind.blade.html` or `simple-tailwind.blade.html` in the same directory. Custom views receive `paginator` metadata and, for length-aware pagination, the numbered `elements`. Add `previous` and `next` to `lang/{locale}/pagination.json` to localize navigation and JSON link labels.

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
