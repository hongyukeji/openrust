# HTTP Responses

## Creating Responses

Use `Response::json` for a JSON payload and an explicit status. Use `header` or `with_headers` with typed HTTP header values. Use `Response::stream` to send chunks from a Rust stream.

See the [controller example](./controllers.md) for input handling and a JSON response.

### Strings and Arrays {#strings-and-arrays}

A route or controller can return a string directly. OpenRust sends it as HTTP 200 with `text/html; charset=utf-8` and uses the string as the HTML body. Return a view to render and escape template data.

```rust
Route::get("/greeting", || "<p>Hello</p>")?;
Route::get("/numbers", || [1, 2, 3])?;
```

Arrays, vectors, `serde_json::Value`, JSON maps, support collections and JSON-backed `Fluent` bags become HTTP 200 JSON. Ordered collections preserve their keys, so sparse numeric keys produce a JSON object. Use `Response::json` when you need to choose the status explicitly.

### Native HTTP Responses {#native-http-responses}

Route closures and controller actions can return an `http::Response<Body>`. The router adapts it to the framework response and sends it through the same middleware and HTTP response preparation. This also applies to asynchronous handlers and validated FormRequest actions.

Add `http = "1.0"` to the application dependencies to use the native response type directly.

```rust
Route::get("/native", || {
    http::Response::new(openrust::http::Body::from("native"))
})?;
```

Use `Response::from(native)` to open a native response in framework code and `into()` to move it back. Conversion preserves status, repeated headers, protocol version, extensions and the current body stream. The HTTP kernel applies HEAD and bodyless status rules when serving the response.

### Request-aware Responses {#responsable}

Implement `openrust::contracts::support::Responsable` when a returned object needs the current request to build its response. The router calls `to_response` after the action runs, so it can read route parameters and input. Its result goes through the regular middleware and HTTP response preparation.

```rust
use openrust::{
    contracts::support::Responsable,
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Route,
};

struct Report;
impl Responsable for Report {
    async fn to_response(self, request: &mut Request) -> Result<Response, HttpError> {
        Response::json(StatusCode::OK, request.route("id"))
    }
}

Route::get("/reports/{id}", || Report)?;
```

A closure can return the object directly. A controller or validated FormRequest action can return it inside its `Result`. Return an `HttpError` from `to_response` to use the application exception renderer.

### Returning a prepared response early {#prepared-response-exceptions}

Return `Err(response.throw_response())` from a fallible handler or middleware to exit with a prepared `Response` or `RedirectResponse`. The HTTP exception handler sends that response through its `respond` callback. A custom renderer for `HttpResponseException` may override it or consume the original response once with `take_response()`. Prepared responses are skipped by default when reporting exceptions.

Use `abort_response(response)` when you already have a framework or native response. For a request-aware object, use `Err(abort_responsable(value, request).await)`. If `to_response` fails, the original error reaches the exception handler. Use `abort(status, message, headers)` for a status-based error.

Use `with_exception(error)` to attach diagnostics that application code can inspect through `response.exception()`. Attaching an exception only stores response metadata.

### Attaching Headers to Responses {#attaching-headers-to-responses}

Use `without_header` with one typed header name or `without_headers` with a collection of names. Both methods return the response for chaining.

#### Cache Control Middleware {#cache-control-middleware}

Apply `cache.headers` to the routes that need a cache policy. Separate directives with semicolons and write option names in `snake_case`. The middleware applies to successful GET and HEAD responses. With `etag`, it reuses an existing validator or hashes the available response body with XXH3-128, the same digest the framework uses for `xxh128`. Use `etag=value` to provide a validator for a stream without buffering it.

```rust
Route::get_with("/cacheable", cache_response, |route| {
    route.middleware_name("cache.headers:public;max_age=2628000;etag");
    Ok(())
})?;
```

Set `last_modified` to a Unix timestamp or a date expression. A matching `If-None-Match` or `If-Modified-Since` can return `304 Not Modified` with an empty body. When both request validators and an ETag are available, the ETag comparison takes precedence.

## Attaching Cookies to Responses

In a middleware stack containing `AddQueuedCookiesToResponse`, use `Cookie::queue` to attach a cookie after the handler returns. For direct attachment, use `response.cookie(&cookie, request)?`. Place `EncryptCookies` before the queue middleware so it protects both directly attached and queued cookies. The factory inherits path, domain, secure and SameSite defaults from the session configuration. A lifetime of zero creates a browser-session cookie.

```rust
use openrust::{
    cookie::CookieOptions,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Cookie,
};

fn remember(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let cookie = Cookie::make("theme", "dark", 30, &CookieOptions::default())?;
        Cookie::queue(cookie)?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

### Setting and expiring response cookies {#expiring-cookies-early}

Both `Response` and `RedirectResponse` support `with_cookie(&cookie, request)` and `with_cookies([&first, &second], request)`. A cookie replaces an earlier one with the same name, path and domain. Chain `without_cookie("theme", None, None, request)` or `without_cookies(["theme", "contrast"], None, None, request)` to expire cookies. Pass the original path and domain when they differ from the configured defaults.

## JSON business responses {#business-responses}

`ApiResponse` provides a business JSON envelope with `success`, `code` and `message`, plus optional `data`, `meta` and `errors`. It is a business response extension; HTTP status codes still describe the request outcome.

### Configuration and automatic envelopes {#business-response-configuration}

`response_envelope` in `config/api.rs` is enabled by default and reads `API_RESPONSE_ENVELOPE`. Most applications need no environment entry. To disable it, add this to your own `.env`:

```dotenv
API_RESPONSE_ENVELOPE=false
```

When disabled, framework exceptions and ordinary API Resources retain their native format. Explicit `ApiResponse` values still use the business envelope. Restart long-running applications and Workers after changing configuration; rebuild a configuration cache with `./artisan config:cache` if used.

Automatic envelopes apply to requests within the API prefix or for which `expects_json()` is true:

- Default framework JSON exception responses gain business fields. Validation errors retain field `errors`. Custom exception renderers may return their own responses.
- Ordinary `JsonResource` instances and resource collections use the business `data` wrapper and retain resource fields such as `links` and `meta`. Existing top-level fields take precedence.
- JSON:API resources and requests accepting `application/vnd.api+json` bypass automatic business envelopes. Returning an explicit `ApiResponse` remains an application decision.
- Strings, arrays, `Response::json`, downloads, streams and HTML are not automatically wrapped. Health checks are not automatically rewritten either.

See [API Resources](../eloquent/resources.md) and [error handling](./errors.md).

### Success, failure and pagination {#business-response-examples}

When registered in `routes/api.rs`, these routes also receive the configured API prefix, which defaults to `/api`.

```rust
use std::collections::BTreeMap;
use openrust::{
    http::{ApiResponse, PageMeta, StatusCode},
    routing::RoutingError,
    support::facades::Route,
};
use serde_json::json;

fn register() -> Result<(), RoutingError> {
    Route::get("/items", || {
        ApiResponse::success(json!([{"id": 1, "name": "Notebook"}]))
            .with_message("Loaded.")
            .with_meta(PageMeta { page: 1, per_page: 10, total: 1, total_pages: 1 })
    })?;
    Route::post("/items", || {
        ApiResponse::success(json!({"id": 2}))
            .with_message("Created.")
            .with_status(StatusCode::CREATED)
    })?;
    Route::get("/example-validation", || {
        ApiResponse::failure(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed", "Invalid input.")
            .with_errors(BTreeMap::from([("email".into(), vec!["Email is required.".into()])]))
    })
}
```

GET `/api/items` returns HTTP 200:

```json
{
  "success": true,
  "code": "ok",
  "message": "Loaded.",
  "data": [{"id": 1, "name": "Notebook"}],
  "meta": {"page": 1, "per_page": 10, "total": 1, "total_pages": 1}
}
```

POST `/api/items` returns HTTP 201 with `{"success":true,"code":"ok","message":"Created.","data":{"id":2}}`.

GET `/api/example-validation` returns HTTP 422:

```json
{
  "success": false,
  "code": "validation_failed",
  "message": "Invalid input.",
  "errors": {"email": ["Email is required."]}
}
```

`success` defaults to HTTP 200; use a non-success status for `failure`. `with_status` changes the HTTP status only. For ordinary responses with a body, `into_response` rejects a mismatch between the success flag and HTTP status.

`ApiResponse::empty()`, `success(())` and `success(Value::Null)` omit `data`; empty objects, arrays, `0`, `false` and empty strings remain present. HTTP 204, 205, 304 and informational responses have no body. Serialization failures use [error handling](./errors.md).

### Localized messages {#business-response-localization}

The default success message uses `response.ok`; framework exceptions use the corresponding `response.*` keys, resolved using the current request locale and existing fallback rules. Override these entries in `response.json` in the application's language directory. See [localization](../digging-deeper/localization.md) for locale configuration and selection; response handlers need not parse language headers themselves.

Messages passed to `with_message` or `failure` are literal. Use `with_message_key` to translate at final serialization. For example, define this entry in `lang/zh-Hans/messages.json` under the default application language directory and return the response while the current locale is `zh-Hans`:

```json
{"created": "已创建 :name。"}
```

```rust
use std::collections::BTreeMap;
use openrust::http::ApiResponse;

fn localized_response() -> ApiResponse<u64> {
    ApiResponse::success(1).with_message_key(
        "messages.created",
        BTreeMap::from([("name".into(), "Notebook".into())]),
    )
}
```

```json
{"success": true, "code": "ok", "message": "已创建 Notebook。", "data": 1}
```

### Custom fields {#business-response-fields}

`with_field` sets a top-level field, `additional` shallow-merges fields and `without_field` removes a field. Operations run in call order; later operations win and nested objects are replaced as a whole. Standard fields may also be overridden, so keep the public schema consistent in your application. A JSON field named `status` does not change the HTTP status.

This response returns HTTP 200:

```rust
use openrust::http::ApiResponse;
use serde_json::json;

fn custom_response() -> ApiResponse<serde_json::Value> {
    ApiResponse::success(json!({"id": 1}))
        .with_message("Saved.")
        .additional(json!({"trace_id": "first", "code": "saved"}).as_object().unwrap().clone())
        .without_field("trace_id")
        .with_field("trace_id", "request-123")
}
```

```json
{
  "success": true,
  "code": "saved",
  "message": "Saved.",
  "data": {"id": 1},
  "trace_id": "request-123"
}
```


## Redirect responses

Use `Redirect::to` for a URL or `Redirect::route` for a named route. `RedirectOptions` defaults to 302 and accepts response headers. `Redirect::guest` stores the intended URL in the request session, and `Redirect::intended` consumes it and uses a fallback when the session has no intended URL.

Use `Redirect::action` for a [registered controller action](./urls.md#urls-for-controller-actions).

Use `Redirect::refresh` to redirect to the current path without its query, and `Redirect::secure` to generate an HTTPS destination from an application path. `Redirect::away` writes an explicit HTTP(S) URL or relative reference to `Location`. Supply a trusted destination. Use `Redirect::signed_route` and `Redirect::temporary_signed_route` to issue a redirect to an absolute signed URL. `Redirect::back` prefers a same-origin Referer, then the previous session URL, then the supplied fallback. `Redirect::get_intended_url` reads the current intended URL and `Redirect::set_intended_url` sets it explicitly. Both take the current request.

For a route-level redirect, `Router::redirect` and `Router::permanent_redirect` can substitute source parameters in the destination. A missing optional destination segment such as `/articles/{slug?}` is omitted. The source parameter names `destination` and `status` are reserved.

### Redirect URL fragments {#redirect-url-fragments}

Call `with_fragment("details")` to replace the current fragment, or `without_fragment()` to remove it. Both keep the URL path and query, and update the `Location` header and the HTML fallback together. Each returns a `Result` so an invalid redirect target can be handled.

### Same-origin redirects {#same-origin-redirects}

Call `redirect.enforce_same_origin(request, "/fallback", true, true)` to compare the target with the current request. The host must match. The two flags also require the scheme and port to match, respectively. The comparison uses the effective request origin, including trusted proxy settings, and treats an explicit default port like an omitted one. A relative target has no host, so it selects the fallback. Supply a trusted fallback such as a fixed application path.

### Redirecting with flashed session data {#redirecting-with-flashed-session-data}

In a web route with a session, chain `with` to flash a value, `with_input` to retain input, and `with_errors` to attach a named `MessageBag`. The Rust methods take the current mutable request. Return the `RedirectResponse` directly from the handler. Its status, headers and session updates flow through the usual HTTP response handling.

```rust
use openrust::{
    http::{HttpError, HttpFuture, RedirectResponse, Request},
    support::facades::Redirect,
};
use serde_json::json;

fn save_profile(request: &mut Request) -> HttpFuture<'_, Result<RedirectResponse, HttpError>> {
    Box::pin(async move {
        Redirect::to("/profile", Default::default())?
            .with(request, "status", json!("saved"))?
            .only_input(request, &["name"])
            .await
    })
}
```

Pass `None` to `with_input` to read form or JSON fields and query parameters from the request. Uploaded files are excluded. Use `only_input` or `except_input` to select fields by dot path. The next request can read old input with `request.old(Some("name"))` and flashed values from the session.

## Other Response Types {#other-response-types}

### View Responses {#view-responses}

Use `Response::view` to render a prepared view with a status and headers. Pass a view from `view("name")` or select the first available template with the `View::first` facade. Rendering preserves the current request context.

### JSON Responses {#json-responses}

Use `Response::json(StatusCode::CREATED, data)?` to select a status and serialize a value implementing `serde::Serialize`. It sets `Content-Type: application/json`. Use `header` or `with_headers` to attach response headers. Returning a JSON value directly from the route uses HTTP 200.

### JSONP Responses {#jsonp-responses}

Use `Response::jsonp(status, Some(callback), data)` to send a JavaScript callback with JSON data. A callback may use dotted names and literal bracket keys. Invalid callback names produce HTTP 400. Passing `None` returns an ordinary JSON response.

### Markdown Responses {#markdown-responses}

Use `Response::markdown(status, content)` to send Markdown source with `text/markdown; charset=utf-8`. Clients decide how to render it.

### Empty Responses {#empty-responses}

Use `Response::no_content()` for an empty HTTP 204 response. Use `Response::empty(status)` to select another status and `with_headers` to attach headers.

### File Downloads {#file-downloads}

Use `Response::download(path, name, headers, "attachment").await` to send a local file as a download. Pass `Some(name)` to change the download name or `None` to use the file's name. Use `"inline"` to set an inline disposition.

### File Responses {#file-responses}

Use `Response::file(path, headers).await` to serve a local file with its content type and length. Both methods stream a regular file selected by the application and set a public cache policy by default.

The HTTP kernel prepares a single byte range after response middleware. A matching `If-Range` can use a strong ETag or the file's `Last-Modified` header. HEAD sends the full file's metadata without a body. Apply `cache.headers` when a route needs cache revalidation and `304 Not Modified` responses. Malformed and multi-range requests receive the full file.

## Streamed Responses {#streamed-responses}

For a stream of raw chunks, `Response::stream` sends `X-Accel-Buffering: no`.

### Consuming Streamed Responses {#consuming-streamed-responses}

Use the Fetch body reader to read chunks as they arrive. For a streamed JSON document, accumulate the text and parse it when the stream completes.

Use `EventSource` for SSE. Listen for the event name and close the connection on the default end marker.

```javascript
const source = new EventSource('/events');
source.addEventListener('snapshot', (event) => {
    renderSnapshot(JSON.parse(event.data));
});
source.addEventListener('update', (event) => {
    if (event.data === '</stream>') source.close();
});
```

### Streamed JSON Responses {#streamed-json-responses}

Use `Response::stream_json` with `StreamedJson::object` to wrap a stream of rows in a JSON object. `StreamedJson::items` serializes each row as it arrives. Use `StreamedJson::value` for fixed values and `StreamedJson::array` for nested arrays. Use `StreamedJson::entries` to stream key-value pairs into a JSON object.

Streamed JSON and JSON event payloads use default string escaping. SSE JSON preserves non-ASCII text apart from characters that require escaping.

```rust
use futures_util::stream;
use openrust::http::{HttpError, Response, StatusCode, StreamedJson};

let users = stream::iter([Ok::<_, HttpError>(1), Ok(2)]);
let data = StreamedJson::object([("users", StreamedJson::items(users))]);
let response = Response::stream_json(StatusCode::OK, data);
```

### Event Streams (SSE) {#event-streams}

`Response::event_stream` sends `text/event-stream` and disables cache and proxy buffering. `StreamedEvent::message` uses the `update` event. Use `StreamedEvent::new` for named text events and `StreamedEvent::json` for JSON payloads. The default last event carries `</stream>`. Use `Response::event_stream_with_end` to supply a final event or pass `None` to omit it.

Once the headers have been sent, a JSON or download source failure ends the body with a transport error. An SSE source failure in the application is reported through its exception handler. The SSE connection closes without a final event.

### Streamed Downloads {#streamed-downloads}

`Response::stream_download` sends byte chunks as a download. With a filename, it builds `Content-Disposition` using the requested disposition and adds a UTF-8 filename when needed. With `None`, the supplied headers determine the disposition.

## Response Macros {#response-macros}

Register an asynchronous response macro from a service provider's `boot` method. The macro receives the arguments as JSON values and returns an HTTP response.

```rust
use openrust::{
    http::{HttpError, Response as HttpResponse, StatusCode},
    support::facades::Response,
};

Response::r#macro("caps", |arguments| async move {
    let [serde_json::Value::String(value)] = arguments.as_slice() else {
        return Err(HttpError::InvalidRequest);
    };
    HttpResponse::json(StatusCode::OK, value.to_uppercase())
})?;
```

Call it by name from a route or controller.

```rust
Response::call("caps", vec!["hello".into()]).await
```

`has_macro` checks the current factory. `mixin` takes explicitly listed `ResponseMacro` values and a replacement flag. `flush_macros` clears the current application's registry. Each application has its own factory.

## Response facade {#response-facade}

Import `support::facades::Response` inside an application scope. It delegates to the same application's `http::ResponseFactory`, also available through the container name `response`. Returned values remain `http::Response`.

```rust
use openrust::{
    http::{HeaderMap, StatusCode},
    support::facades::{Response, Route},
};

Route::get("/status", || async {
    Response::json(
        serde_json::json!({"status": "ready"}),
        StatusCode::OK,
        HeaderMap::new(),
    )
})?;
```

`make` accepts strings or structured response values. `no_content`, `json`, `jsonp`, and `markdown` take an explicit status and headers. `stream` accepts a Rust asynchronous stream; `stream_json` accepts `StreamedJson`. `event_stream` appends `</stream>` by default; `event_stream_with_end(..., None)` disables the final event. Use `file`, `download`, and `stream_download` for files and streamed downloads.

`view` renders with the current request; `view_first` chooses the first existing view. A handler borrowing `&mut Request` can use `view_for_request`, or lend the request through `request.with_container(async { ... }).await`. `redirect_to`, `redirect_to_route`, and `redirect_to_action` reuse the application routes and URL generator. `redirect_guest` and `redirect_to_intended` modify the current request's Session.
