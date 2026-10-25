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
- [`middleware::apply`] — request id, tracing span, timeout and panic capture, in a fixed order.

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

`PageParams` defaults to `page=1&per_page=20`; extraction accepts `page >= 1` and `1 <= per_page <= 100`. Invalid values return a 422 `application/problem+json` response with field-level `errors`. `offset()` is zero-based and `limit()` returns the page size. `Paginated::new(items, params, total)` emits `{"data": [...], "meta": {"page": 1, "per_page": 20, "total": 57, "total_pages": 3}}`.
