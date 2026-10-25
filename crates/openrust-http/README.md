# openrust-http

HTTP layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework, built on `axum` and `tower`.

- [`HttpProvider`] — registers the `serve` command, the default middleware stack and `/health`.
- [`AppBuilderHttpExt::routes`] — the hook an application uses to contribute routes.
- [`Routes`] — thin routing convention over `axum::Router`: `get`/`post`/…, `group(prefix, …)`, `merge`, `layer`.
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
