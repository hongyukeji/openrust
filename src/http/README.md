# HTTP

HTTP layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework, built on `axum` and `tower`.

- [`HttpPlugin`] — install with `app.plugin(HttpPlugin)` to register the `serve` command, the default middleware stack and `/health`.
- [`AppBuilderHttpExt::routes`] — the hook an application uses to contribute routes.
- [`AppBuilderHttpExt::http_layer`] — registers shared Tower request context around every route, outside route-level layers.
- [`Routes`] — thin routing convention over `axum::Router`: `get`/`post`/…, `nest(prefix, …)`, `merge`, `layer`.
- [`Resource`] and [`ResourceActions`] — register only the controller actions that exist; `Routes::resource` maps `list`/`create` to the collection and `get`/`update`/`delete` to `{id}`. `resource_with` filters them with `only` or `except`.
- [`Created<T>`] and [`NoContent`] — a success envelope (201 in semantic mode) with optional `Location`, and a success envelope with `data: null` (HTTP 200).
- [`PageParams`] and [`Paginated<T>`] — validated `page`/`per_page` query parameters and an API envelope with array `data` and pagination `meta`.
- [`SortParams`] — parses ADR-008 comma-separated sort syntax only against an explicit field allowlist; applications map names to fixed ORM columns.
- [`Json<T>`], [`Path<T>`] and [`Query<T>`] — axum-compatible extractors with API envelope rejections. `Json<T>` also implements `IntoResponse`.
- [`Validated`] — runs `validator` after the OpenRust `Json<T>` or `Query<T>` extractor succeeds.
- [`Inject`] — extractor that fetches a service registered with `AppBuilder::provide`.
- [`ClientIp`] — resolves the socket peer by default and accepts `X-Forwarded-For` only from configured trusted proxy IPs/CIDRs.
- [`RateLimitLayer`] — explicitly rate-limits selected routes through the registered shared `RateLimiter`; IP keys honor `ClientIp` trusted proxy rules, authenticated keys require an outer `AuthLayer`, and custom keys receive the request plus a policy scope.
- [`middleware::apply`] — request id, tracing span, CORS, timeout, gzip/br compression and panic capture, from outermost to innermost. `build_router` injects the final `AppState` into request extensions before route layers execute, so capability middleware can use the application registry. It returns a configuration error before listening if enabled CORS settings are invalid.

`[server.compression] enabled = true` negotiates gzip and Brotli using `Accept-Encoding`; set it to `false` to disable compression. `[server.cors]` is disabled by default. To enable it, set `enabled = true` and `allowed_origins = ["https://example.com"]` (or `["*"]` without credentials). Methods, request headers, exposed headers, credentials and preflight `max_age_secs` are configurable. Concrete origins must be `http://` or `https://` plus a host and optional port, without a path, query or trailing slash. Wildcards cannot be mixed with concrete values in any CORS list; credentials also forbid wildcard methods, request headers and exposed headers. Invalid settings return a keyed configuration error with the escaped value during router construction. CORS wraps the timeout layer, so allowed origins remain visible on 408 responses. `build_router(&App)` returns `Result<Router>`; `serve` builds the router before binding its listener.

`[server].trusted_proxy_ranges` is empty by default. Entries accept an IP address or CIDR. When the direct socket peer is in a configured range, `ClientIp` parses `X-Forwarded-For` and walks from right to left across trusted proxy hops; it returns the first untrusted address. Otherwise it ignores forwarding headers. Invalid trusted ranges fail router construction, and malformed forwarded values from a trusted peer return a bad-request envelope.

Use a route layer to apply a shared fixed-window quota. The application must register a `RateLimiter`, normally via `DatabasePlugin`; storage failures fail closed. Each scope should identify one endpoint/policy and use one stable limit/window across every route instance. `by_user` requires the authentication layer to be added first so it executes before rate limiting; `by_user_or_client_ip` can also be used on routes where anonymous requests are allowed.

```rust,ignore
use std::time::Duration;
use openrust::http::{AuthLayer, AuthMode, RateLimitLayer, Routes};

fn routes(routes: Routes) -> Routes {
    routes.get("/api/profile", profile)
        .layer(RateLimitLayer::by_user("profile:read", 60, Duration::from_secs(60)).layer())
        .layer(AuthLayer::required(AuthMode::Jwt))
}
```

Each subsequent route layer wraps the earlier one. In this example, `AuthLayer` is outermost and runs first, then the rate-limit layer reads the verified account identity. Only routes covered by the layer are limited; health and fallback routes are unaffected unless explicitly wrapped. Custom key functions should return a bounded, stable, non-secret identity value.

```rust
use openrust::http::{AppBuilderHttpExt, Inject, Json, Routes, Validated};
use openrust::core::{AppBuilder, Result};
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
use openrust::http::{Action, Resource, ResourceActions, Routes};

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

### Data output and query boundaries

Return an application DTO instead of serializing a SeaORM model. The DTO is the response field allowlist; never derive `Serialize` for a model that contains password hashes, internal notes, or other private columns. Use a typed query struct with `#[serde(deny_unknown_fields)]` so misspelled and unsupported filters fail. Map each filter to a fixed SeaORM column and validate `sort` before mapping it to columns:

```rust,ignore
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PostQuery {
    status: Option<String>,
    sort: Option<String>,
}

#[derive(serde::Serialize)]
struct PostResponse {
    id: i64,
    title: String,
}

let sort = SortParams::parse(query.sort.as_deref(), &["created_at", "title"])?;
let mut select = post::Entity::not_deleted();
if let Some(status) = query.status {
    select = select.filter(post::Column::Status.eq(status));
}
for field in sort.fields() {
    let order = match field.direction() {
        SortDirection::Ascending => sea_orm::Order::Asc,
        SortDirection::Descending => sea_orm::Order::Desc,
    };
    let column = match field.name() {
        "created_at" => post::Column::CreatedAt,
        "title" => post::Column::Title,
        _ => return Err(openrust::Error::BadRequest("unsupported sort field".into())),
    };
    select = select.order_by(column, order);
}
```

`SortParams` rejects unsupported, duplicate, malformed and overlong values without echoing request data. It does not build SQL or decide authorization. Query DTO fields form the filter allowlist; build SeaORM `Condition`s explicitly, never interpolate request field names into SQL.

`openrust generate model post --soft-delete title:string` and the matching `generate migration posts --soft-delete title:string` add nullable `deleted_at` and an explicit `Entity::not_deleted()` scope. The scope is opt-in; direct `Entity::find()` and raw SQL include deleted rows. Restore by setting `deleted_at` to `None` in an `ActiveModel`. `--uuid-id` on both commands opts into `UUIDv7` primary keys; existing projects keep integer IDs by default.

For relations, define SeaORM `Relation`/`Related` edges for generated foreign keys, then eager-load a collection with `Entity::find_with_related(related::Entity)` or page parents first and call SeaORM 2 `load_many(related::Entity, db)`. Avoid querying each parent inside a loop. Apply authorization, soft-delete and tenant conditions to each relevant entity explicitly; relation loading does not infer those application policies. Page parents before batch loading to bound both parent and child work.

Request parsing follows [ADR-008](../../.agents/project/decisions/adr-008-api-conventions.md): malformed JSON, path and query values use `bad_request` (semantic HTTP 400); missing or non-JSON `Content-Type` uses `unsupported_media_type` (415); structurally valid JSON with missing or mistyped fields uses `validation_failed` (422) with a readable `message` and field `errors` when available; bodies over axum's default 2 MiB limit use `payload_too_large` (413). Validation rules also use 422. Unmatched routes use `not_found` (404), unsupported methods use `method_not_allowed` (405) with `Allow`, and timeouts use `request_timeout` (408). Parenthesized HTTP statuses apply in `semantic` mode. All are `application/json` envelopes with `x-request-id`; enabled CORS applies to these responses. CORS preflight requests (`OPTIONS` with `Access-Control-Request-Method`) keep tower-http's empty body on both allowed and rejected responses; browsers use only the protocol response headers. Ordinary cross-origin errors remain wrapped and include CORS headers for allowed origins. Handler-generated client errors retain their own message. Axum's original extractors remain available through [`axum`].

`Created::with_location("/api/posts/42", Json(post))?` returns a `Located<T>` response. Invalid `Location` header values return an `Error` that renders as an `internal_error` envelope (semantic HTTP 500); construction never panics. `Created(Json(post))` remains available when no location is needed.

### Outbound HTTP client

Enable the optional `http-client` feature to use `openrust::http::HttpClient`. A client clone shares its connection pool. Defaults are a 5-second connection timeout, 30-second total timeout, 1 MiB maximum response body, and no automatic redirects. JSON, URL-encoded form, multipart and streaming request bodies are supported; response status is returned as data and is not treated as a transport error. `HttpResponse::next_chunk` streams response data while enforcing the configured total body limit. Dropping the request future cancels local waiting, but cannot undo work already received by a remote server.

```rust,ignore
use openrust::http::{HttpClient, HttpClientConfig};

let client = HttpClient::new(HttpClientConfig::default())?;
let response = client.get("https://api.example.test/status").send().await?;
if response.status().is_success() {
    let value: serde_json::Value = response.json().await?;
}
```

The client does not retry automatically. To retry, the application must explicitly mark the operation with `idempotent()` and pass a `RetryPolicy`; policy allows at most five attempts and uses capped exponential backoff for transport failures and 502/503/504 responses. A request with a one-shot streaming body is rejected when replay is required. Redirects are disabled to avoid implicit target changes; this does not replace application-side SSRF destination validation. System proxy environment variables are ignored; configure `HttpClientConfig::proxy` explicitly when a proxy is required. `HttpTransport` and `TransportResponse::from_bytes` support deterministic fakes; injected transports receive built requests and do not use the network unless their implementation chooses to. Transport and decoding errors redact underlying details, and applications should not log authorization headers or sensitive bodies. Use the returned status to handle non-2xx responses explicitly.

```rust,ignore
use std::time::Duration;
use openrust::http::{HttpClient, HttpClientConfig, RetryPolicy};

let client = HttpClient::new(HttpClientConfig::default())?;
let policy = RetryPolicy::new(3, Duration::from_millis(100), Duration::from_secs(1))?;
let response = client
    .get("https://api.example.test/idempotent-resource")
    .idempotent()
    .send_with_retry(policy)
    .await?;
```

`[api] http_status = "always_ok"` is the default: all framework JSON envelopes return HTTP 200. `semantic` retains the status table from ADR-008. The middleware stores the selected mode and request ID in request-scoped task context so success, extraction errors, fallbacks, timeouts, and panic responses share one policy. The JSON `/health` response uses the same envelope; non-JSON responses such as files, streams, and HTML remain raw.
