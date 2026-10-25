# openrust-http

HTTP layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework, built on `axum` and `tower`.

- [`HttpPlugin`] — install with `app.plugin(HttpPlugin)` to register the `serve` command, the default middleware stack and `/health`.
- [`AppBuilderHttpExt::routes`] — the hook an application uses to contribute routes.
- [`Routes`] — thin routing convention over `axum::Router`: `get`/`post`/…, `nest(prefix, …)`, `merge`, `layer`.
- [`Resource`] and [`ResourceActions`] — register only the controller actions that exist; `Routes::resource` maps `list`/`create` to the collection and `get`/`update`/`delete` to `{id}`. `resource_with` filters them with `only` or `except`.
- [`Created<T>`] and [`NoContent`] — a success envelope (201 in semantic mode) with optional `Location`, and a success envelope with `data: null` (HTTP 200).
- [`PageParams`] and [`Paginated<T>`] — validated `page`/`per_page` query parameters and an API envelope with array `data` and pagination `meta`.
- [`Json<T>`], [`Path<T>`] and [`Query<T>`] — axum-compatible extractors with API envelope rejections. `Json<T>` also implements `IntoResponse`.
- [`Validated`] — runs `validator` after the OpenRust `Json<T>` or `Query<T>` extractor succeeds.
- [`Inject`] — extractor that fetches a service registered with `AppBuilder::provide`.
- [`middleware::apply`] — request id, tracing span, CORS, timeout, gzip/br compression and panic capture, from outermost to innermost. It returns a configuration error before listening if enabled CORS settings are invalid.

`[server.compression] enabled = true` negotiates gzip and Brotli using `Accept-Encoding`; set it to `false` to disable compression. `[server.cors]` is disabled by default. To enable it, set `enabled = true` and `allowed_origins = ["https://example.com"]` (or `["*"]` without credentials). Methods, request headers, exposed headers, credentials and preflight `max_age_secs` are configurable. Concrete origins must be `http://` or `https://` plus a host and optional port, without a path, query or trailing slash. Wildcards cannot be mixed with concrete values in any CORS list; credentials also forbid wildcard methods, request headers and exposed headers. Invalid settings return a keyed configuration error with the escaped value during router construction. CORS wraps the timeout layer, so allowed origins remain visible on 408 responses. `build_router(&App)` returns `Result<Router>`; `serve` builds the router before binding its listener.

```rust
use openrust_http::{AppBuilderHttpExt, Inject, Json, Routes, Validated};
use openrust_core::{AppBuilder, Result};
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
struct CreatePost { #[validate(length(min = 1, max = 120))] title: String }

struct Posts;

async fn create(Inject(_posts): Inject<Posts>, Validated(Json(input)): Validated<Json<CreatePost>>) -> Result<Json<String>> {
    Ok(Json(input.title))
}

fn register(app: &mut AppBuilder) {
    app.insert(Posts).routes(|r| r.nest("/api", |api| api.post("/posts", create)));
}
```

Resource controllers explicitly list their handlers; missing actions do not create a path. Filtering is applied after the controller registers its actions:

```rust
use openrust_http::{Action, Resource, ResourceActions, Routes};

struct Posts;
async fn list() -> &'static str { "posts" }
async fn get() -> &'static str { "post" }

impl Resource for Posts {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions.list(list).get(get)
    }
}

fn routes(routes: Routes) -> Routes {
    routes.nest("/api", |api| {
        api.resource_with::<Posts>("/posts", |a| a.only(&[Action::List]))
    })
}
```

Resource paths discard trailing slashes before registering the collection and `{id}` member routes; an empty path inside `nest("/posts", …)` responds at `/posts` and `/posts/{id}`. Repeated `only` calls keep the intersection of their action lists. Axum panics at registration on route conflicts, including a member parameter name other than `id` at the same path position.

`PageParams` defaults to `page=1&per_page=20`; direct extraction accepts `page >= 1`, `1 <= per_page <= 100`, and an offset no larger than `i64::MAX`. Invalid values return a `validation_failed` API envelope with field-level `errors`. `Query<PageParams>` only deserializes and does not validate these bounds. `offset()` is zero-based and `limit()` returns the page size. `Paginated::new(items, params, total)` emits `{"success": true, "code": "ok", "message": "ok", "data": [...], "meta": {"page": 1, "per_page": 20, "total": 57, "total_pages": 3}, "request_id": "..."}` using typed `PageMeta`.

Request parsing follows [ADR-008](../../.agents/project/decisions/adr-008-api-conventions.md): malformed JSON, path and query values use `bad_request` (semantic HTTP 400); missing or non-JSON `Content-Type` uses `unsupported_media_type` (415); structurally valid JSON with missing or mistyped fields uses `validation_failed` (422) with a readable `message` and field `errors` when available; bodies over axum's default 2 MiB limit use `payload_too_large` (413). Validation rules also use 422. Unmatched routes use `not_found` (404), unsupported methods use `method_not_allowed` (405) with `Allow`, and timeouts use `request_timeout` (408). Parenthesized HTTP statuses apply in `semantic` mode. All are `application/json` envelopes with `x-request-id`; enabled CORS applies to these responses. CORS preflight requests (`OPTIONS` with `Access-Control-Request-Method`) keep tower-http's empty body on both allowed and rejected responses; browsers use only the protocol response headers. Ordinary cross-origin errors remain wrapped and include CORS headers for allowed origins. Handler-generated client errors retain their own message. Axum's original extractors remain available through [`axum`].

`Created::with_location("/api/posts/42", Json(post))?` returns a `Located<T>` response. Invalid `Location` header values return an `Error` that renders as an `internal_error` envelope (semantic HTTP 500); construction never panics. `Created(Json(post))` remains available when no location is needed.

`[api] http_status = "always_ok"` is the default: all framework JSON envelopes return HTTP 200. `semantic` retains the status table from ADR-008. The middleware stores the selected mode and request ID in request-scoped task context so success, extraction errors, fallbacks, timeouts, and panic responses share one policy. The JSON `/health` response uses the same envelope; non-JSON responses such as files, streams, and HTML remain raw.
