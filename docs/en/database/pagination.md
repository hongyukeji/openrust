# Database Pagination

## Basic Usage

`paginate(15)` runs a total count and fetches one page. Use `simple_paginate(15)` when only previous and next links are needed. It fetches one extra row to detect a following page. Both work on Query Builder and Eloquent. Eloquent defaults to primary-key order. Set an explicit order on Query Builder for stable pages.

Use `paginate_with` or `simple_paginate_with` with `PageOptions` to change the page name, select columns or supply an explicit page number. HTTP uses the current request’s query string. Outside HTTP, the default page is 1.

Declare a model’s default page size with `ModelMetadata::per_page(size)`. Eloquent and relationship queries use the selected model’s setting when `per_page` is omitted. An explicit page size takes precedence. Query Builder defaults to 15. For length-aware pagination, set `PageOptions::total` to a precomputed count to skip the count query. A zero total returns an empty page without fetching rows. If the total comes from a cache or another service, resolve it before building the options.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn page_with_cached_total<M: Model>(total: u64) -> DatabaseResult<LengthAwarePaginator<M>> {
    M::query()?.paginate_with(PageOptions {
        total: Some(total),
        ..Default::default()
    }).await
}
```

Length-aware pagination also accepts callbacks in `PageOptions`. Use `total_using` to supply a total or return `Ok(None)` to count rows. An explicit numeric `total` wins. Once the total is known, `per_page_using` receives it and can choose the size or return `Ok(None)` to keep the explicit or model default size. A callback error stops pagination before row retrieval.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn adaptive_page<M: Model>() -> DatabaseResult<LengthAwarePaginator<M>> {
    M::query()?.paginate_with(PageOptions {
        per_page_using: Some(&|total| Ok((total > 100).then_some(50))),
        ..Default::default()
    }).await
}
```

## Cursor Pagination

`cursor_paginate(15)` compares the values of ordered columns and fetches one extra row. Pass the returned `next_cursor` or `prev_cursor` in the request. In Query Builder, order by selected, non-null scalar columns with a unique final tie-breaker. An Eloquent query without an explicit order uses the primary key. Grouped queries, unions and raw order expressions use offset pagination.

## Manually Creating a Paginator {#manually-creating-a-paginator}

When the records are already in memory, pass the current slice to `LengthAwarePaginator::new` along with the total before slicing. For `Paginator::new`, include one extra item when another page exists. For `CursorPaginator::new`, supply the ordered slice with the same look-ahead item, plus a callback that extracts each boundary item’s named sort keys. For a previous-page cursor, supply items in reverse fetch order. The paginator restores display order.

```rust
use openrust::{
    database::Value,
    pagination::{Cursor, CursorPaginator, PaginationResult, PaginatorOptions},
};

fn manual_cursor_page(
    ordered: Vec<i64>,
    current: Option<&Cursor>,
) -> PaginationResult<CursorPaginator<String>> {
    let mut page = CursorPaginator::new(
        ordered,
        2,
        current,
        PaginatorOptions {
            cursor_name: Some("position"),
            ..Default::default()
        },
        |id| [("id", Value::Integer(*id))],
    )?;
    page.with_query_string();
    Ok(page.through(|id| id.to_string()))
}
```

## Paginator Instance Methods {#paginator-instance-methods}

Use `count()`, `on_first_page()` and `on_last_page()` for page state. `through` consumes the paginator, transforms the items and preserves the pagination metadata even when the item type changes. The cursor keeps the original sort keys for navigation.

Use `with_path` to change the URL base, `set_page_name` or `set_cursor_name` to change the query parameter name, `with_query_string` to retain request filters and `fragment` to append an anchor. `get_options()` exposes the effective link settings.

When a response has multiple paginators, give each one a distinct query parameter. For example, use `articles` and `comments` in their respective `PageOptions.page_name` fields. When generated links keep the request query, each paginator replaces its own parameter and retains the other page and filters. Use `appends` to add specific filters and `fragment` to link to the corresponding results section.

## Paginating Related Models {#paginating-related-models}

Call `paginate`, `simple_paginate`, or `cursor_paginate` on a `HasMany` or `MorphMany` descriptor with the parent model and page size. The `With` variants accept the same options as Eloquent queries. The related query retains the parent key, the morph type where applicable, configured filters, and model scopes. A parent without a key yields an empty page.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasMany}},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn related_comments<P: Model, R: Model>(
    article: &P,
    comments: &HasMany<P, R>,
) -> DatabaseResult<LengthAwarePaginator<R>> {
    comments.paginate_with(article, PageOptions {
        per_page: Some(15),
        page_name: "comments",
        ..Default::default()
    }).await
}
```

The same three methods are available on `HasOne`, `MorphOne`, `BelongsTo` and `MorphTo`. A one-of-many relation keeps its existing selection. These pages contain persisted records. An absent inverse reference produces an empty page with the requested pagination metadata. `MorphTo` uses the registered target selected by the morph type and returns a page of `MorphTarget` values.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::{MorphTarget, MorphTo}}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn subject_page<P: Model>(
    child: &P,
    relation: &MorphTo<P>,
) -> DatabaseResult<CursorPaginator<MorphTarget>> {
    relation.cursor_paginate_with(child, CursorOptions {
        per_page: Some(15),
        cursor_name: "subjects",
        ..Default::default()
    }).await
}
```

`HasManyThrough` and `HasOneThrough` also accept these pagination methods. The query applies the intermediate join and excludes soft-deleted intermediate models by default. Call `with_trashed_parents()` to include them. The default projection contains only columns from the related model. Qualify custom sort columns when the joined tables share the same column name.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasManyThrough}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn through_page<P: Model, I: Model, R: Model>(
    parent: &P,
    relation: &HasManyThrough<P, I, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation.cursor_paginate_with(parent, CursorOptions {
        per_page: Some(15),
        cursor_name: "reactions",
        ..Default::default()
    }).await
}
```

`BelongsToMany` and `MorphToMany` support the same paginator methods. Each model on the current page carries its Pivot, including any custom model or accessor. Cursor pagination selects columns used by `order_by_pivot` for cursor generation. Use `with_pivot` to expose additional columns on the Pivot. The ordered columns together must uniquely identify each row.

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
        .with_pivot(["label"])
        .order_by_pivot("weight", "asc")
        .order_by_pivot("label_id", "asc")
        .cursor_paginate(parent, 15)
        .await
}
```

### Collection Keys on Eloquent Pages {#collection-keys-on-eloquent-pages}

Eloquent calls `Model::new_collection` after hydrating the current page and loading its relations. Numbered and simple pages preserve customized keys in JSON and expose `get_by_key`. Cursor pages reindex to a list. `items()` always supports ordered iteration.

```rust
use openrust::{
    database::eloquent::Model,
    pagination::LengthAwarePaginator,
};

fn find_on_page<'a, M: Model>(
    page: &'a LengthAwarePaginator<M>,
    key: &str,
) -> Option<&'a M> {
    page.get_by_key(key)
}
```

### Loading Polymorphic Relations on the Current Page {#loading-polymorphic-relations-on-the-current-page}

Use `load_morph` or `load_morph_count` on an Eloquent paginator to load a nested relation or its count for one registered morph target. The methods update only the current page and return the same paginator for chaining.

```rust
use openrust::{
    database::{
        DatabaseResult,
        eloquent::{Model, relations::{ExistenceRelation, MorphTo}},
    },
    pagination::LengthAwarePaginator,
};

async fn load_page_subjects<M: Model, T: Model>(
    page: &mut LengthAwarePaginator<M>,
    subject: &MorphTo<M>,
    comments: &impl ExistenceRelation<Parent = T>,
) -> DatabaseResult<()> {
    page.load_morph("subject", subject, "article", "comments", comments)
        .await?
        .load_morph_count("subject", subject, "article", "comments", comments)
        .await?;
    Ok(())
}
```

## Displaying Pagination Results {#displaying-pagination-results}

Use the paginator’s `links()` to create a Blade view. The default Tailwind view shows the result range and numbered links for `paginate`. Both `simple_paginate` and `cursor_paginate` show previous and next links. Navigation is empty when all results fit on one page. `on_each_side` adjusts the numbered window.

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

Use `links_with("articles.navigation", data)` for a custom view, or override the default at `resources/views/vendor/pagination/tailwind.blade.html` or `simple-tailwind.blade.html` in the same directory. Custom views receive `paginator` metadata and, for length-aware pagination, the numbered `elements`. Add `previous` and `next` to `lang/{locale}/pagination.json` to localize navigation and JSON link labels.

## Customizing the Pagination View {#customizing-the-pagination-view}

Set `Paginator::default_view("articles.navigation")` for numbered pagination and `Paginator::default_simple_view("articles.simple-navigation")` for simple and cursor pagination in your application service provider’s `boot` hook. The view defaults apply to the current application. A named view passed to `links_with` takes precedence.

Use `Paginator::use_bootstrap_five()`, `use_bootstrap_four()` or `use_bootstrap_three()` to select both templates. `use_bootstrap()` selects Bootstrap 4, and `use_tailwind()` restores the Tailwind defaults. Include the corresponding CSS in your application.

```rust
use openrust::{
    foundation::Application,
    http::HttpFuture,
    pagination::Paginator,
    support::{ProviderResult, ServiceProvider},
};

struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    fn boot(&mut self, _: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async {
            Paginator::use_bootstrap_five()?;
            Ok(())
        })
    }
}
```

Select `pagination::semantic-ui` explicitly for numbered Semantic UI links. All nine built-in templates are published to `resources/views/vendor/pagination`. Edit a published template to override the built-in version. Add `--force` to replace existing files.

```bash
./artisan vendor:publish --tag=laravel-pagination
```

## Pagination Windows and JSON {#pagination-windows-and-json}

Use `UrlWindow::make(&page)` to get the opening URLs, sliding window, and closing URLs selected by `on_each_side`. Each segment has ordered `(page, url)` pairs. `link_collection()` gives the flattened numbered links, ellipses, and translated previous and next labels.

```rust
use openrust::pagination::{LengthAwarePaginator, UrlWindow, UrlWindowLinks};

fn numbered_links<T>(page: &LengthAwarePaginator<T>) -> UrlWindowLinks {
    UrlWindow::make(page)
}
```

`to_array()` returns the JSON object. `to_json()` and `to_pretty_json()` encode the same metadata and records. Manual `LengthAwarePaginator::new` accepts a page size of zero and uses one as the divisor for `last_page()`. Query pagination still requires a positive page size.

## JSON and Links

Paginators serialize when their items implement Serde’s `Serialize`. Use `map` to convert rows or Eloquent models into your response items without discarding page metadata. Call `with_query_string` to retain filters or `appends` to add selected parameters. Links use request-relative paths.

```rust
use openrust::database::{Db, DatabaseResult};

async fn article_ids(table: &str) -> DatabaseResult<impl serde::Serialize> {
    let mut page = Db::table(table)?
        .order_by("id", "asc")
        .paginate(15)
        .await?;
    page.with_query_string();
    Ok(page.map(|row| match row.get("id") {
        Some(openrust::database::Value::Integer(id)) => Some(id.to_string()),
        Some(openrust::database::Value::Unsigned(id)) => Some(id.to_string()),
        _ => None,
    }))
}
```

## Customizing pagination resolvers {#customizing-pagination-resolvers}

Register `Paginator::current_page_resolver`, `current_path_resolver`, and `query_string_resolver` in your provider's `boot` method. `LengthAwarePaginator` shares these settings. Use `CursorPaginator::current_cursor_resolver` for cursor pagination. Each callback receives the configured parameter name where applicable; return `None` from a cursor resolver to start at the first page.

```rust
Paginator::current_page_resolver(|name| if name == "entries" { 2 } else { 1 })?;
Paginator::current_path_resolver(|| "/articles".to_owned())?;
Paginator::query_string_resolver(|| vec![("category".into(), "rust".into())])?;
```

The resolvers affect manual paginators, query builders, models, and relationship pagination in the current application. Explicit page, cursor, and path options take precedence. `with_query_string()` invokes the query resolver when called, replaces previously appended values with matching keys, and excludes the paginator's position parameter. The query callback returns string pairs or `None`. `None` and an empty list both suppress request query parameters; `resolve_query_string` preserves their distinction and does not substitute its fallback for an explicit `None`. Nested query values must be encoded by the application.

`resolve_current_page(name, default)`, `resolve_current_path(default)`, `resolve_query_string(default)`, and `CursorPaginator::resolve_current_cursor(name, default)` expose the same resolution. Outside an application request, missing callbacks return the supplied defaults. With the default request source, invalid or missing pages start at one and invalid cursors start at the beginning. The default path remains relative to the request.

`Paginator::view_factory_resolver` accepts a callback returning `ViewResult<Arc<openrust::view::Factory>>`. `Paginator::view_factory`, `LengthAwarePaginator::view_factory`, and `CursorPaginator::view_factory` return the shared factory; pagination view rendering uses it. Callbacks must be `Send + Sync + 'static`; register them in the active application's scope. Registration returns an error when no application container is available.
