# openrust-http

HTTP layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework, built on `axum` and `tower`.

- [`HttpProvider`] — registers the `serve` command, the default middleware stack and `/health`.
- [`AppBuilderHttpExt::routes`] — the hook an application uses to contribute routes.
- [`Routes`] — thin routing convention over `axum::Router`: `get`/`post`/…, `group(prefix, …)`, `merge`, `layer`.
- [`Resource`] and [`ResourceActions`] — register only the controller actions that exist; `Routes::resource` maps `index`/`store` to the collection and `show`/`update`/`destroy` to `{id}`. `resource_with` filters them with `only` or `except`.
- [`Created<T>`] and [`NoContent`] — 201 with the wrapped response, and 204 with an empty body.
- [`PageParams`] and [`Paginated<T>`] — validated `page`/`per_page` query parameters and a JSON response with `data` and `meta`.
- [`Validated`] — extractor that deserialises `Json<T>` or `Query<T>` and runs `validator` rules, answering `422` problem documents on failure.
- [`Inject`] — extractor that fetches a service registered with `AppBuilder::provide`.
- [`middleware::apply`] — request id, tracing span, CORS, timeout, gzip/br compression and panic capture, from outermost to innermost. It returns a configuration error before listening if enabled CORS settings are invalid.

`[server.compression] enabled = true` negotiates gzip and Brotli using `Accept-Encoding`; set it to `false` to disable compression. `[server.cors]` is disabled by default. To enable it, set `enabled = true` and `allowed_origins = ["https://example.com"]` (or `["*"]` without credentials). Methods, request headers, exposed headers, credentials and preflight `max_age_secs` are configurable. Concrete origins must be `http://` or `https://` plus a host and optional port, without a path, query or trailing slash. Wildcards cannot be mixed with concrete values in any CORS list; credentials also forbid wildcard methods, request headers and exposed headers. Invalid settings return a keyed configuration error with the escaped value during router construction. CORS wraps the timeout layer, so allowed origins remain visible on 408 responses. `build_router(&App)` returns `Result<Router>`; `serve` builds the router before binding its listener.

```rust
use openrust_http::{AppBuilderHttpExt, Inject, Routes, Validated};
use openrust_http::axum::Json;
use openrust_core::{AppBuilder, Result};
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
struct CreatePost { #[validate(length(min = 1, max = 120))] title: String }

struct Posts;

async fn store(Inject(_posts): Inject<Posts>, Validated(Json(input)): Validated<Json<CreatePost>>) -> Result<Json<String>> {
    Ok(Json(input.title))
}

fn register(app: &mut AppBuilder) {
    app.provide(Posts).routes(|r| r.group("/api", |api| api.post("/posts", store)));
}
```

Resource controllers explicitly list their handlers; missing actions do not create a path. Filtering is applied after the controller registers its actions:

```rust
use openrust_http::{Action, Resource, ResourceActions, Routes};

struct Posts;
async fn index() -> &'static str { "posts" }
async fn show() -> &'static str { "post" }

impl Resource for Posts {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions.index(index).show(show)
    }
}

fn routes(routes: Routes) -> Routes {
    routes.group("/api", |api| {
        api.resource_with::<Posts>("/posts", |a| a.only(&[Action::Index]))
    })
}
```

Resource paths discard trailing slashes before registering the collection and `{id}` member routes; an empty path inside `group("/posts", …)` responds at `/posts` and `/posts/{id}`. Repeated `only` calls keep the intersection of their action lists. Axum panics at registration on route conflicts, including a member parameter name other than `id` at the same path position.

`PageParams` defaults to `page=1&per_page=20`; direct extraction accepts `page >= 1`, `1 <= per_page <= 100`, and an offset no larger than `i64::MAX`. Invalid values return a 422 `application/problem+json` response with field-level `errors`. `Query<PageParams>` only deserializes and does not validate these bounds. `offset()` is zero-based and `limit()` returns the page size. `Paginated::new(items, params, total)` emits `{"data": [...], "meta": {"page": 1, "per_page": 20, "total": 57, "total_pages": 3}}`.
