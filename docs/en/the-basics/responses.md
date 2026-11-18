# HTTP Responses

## Creating Responses

Use `Response::json` for a JSON payload and an explicit status. Use `header` or `withHeaders` with typed HTTP header values. Use `Response::stream` to send chunks from a Rust stream.

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

Implement `openrust::contracts::support::Responsable` when a returned object needs the current request to build its response. The router calls `toResponse` after the action runs, so it can read route parameters and input. Its result goes through the regular middleware and HTTP response preparation.

```rust
use openrust::{
    contracts::support::Responsable,
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Route,
};

struct Report;

#[allow(non_snake_case)]
impl Responsable for Report {
    async fn toResponse(self, request: &mut Request) -> Result<Response, HttpError> {
        Response::json(StatusCode::OK, request.route("id"))
    }
}

Route::get("/reports/{id}", || Report)?;
```

A closure can return the object directly. A controller or validated FormRequest action can return it inside its `Result`. Return an `HttpError` from `toResponse` to use the application exception renderer.

### Attaching Headers to Responses {#attaching-headers-to-responses}

Use `withoutHeader` with one typed header name or `withoutHeaders` with a collection of names. Both methods return the response for chaining.

#### Cache Control Middleware {#cache-control-middleware}

Apply `cache.headers` to the routes that need a cache policy. Separate directives with semicolons and write option names in `snake_case`. The middleware applies to successful GET and HEAD responses. With `etag`, it reuses an existing validator or hashes the available response body with XXH3-128, the same digest Laravel uses for `xxh128`. Use `etag=value` to provide a validator for a stream without buffering it.

```rust
Route::getWith("/cacheable", cache_response, |route| {
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

## JSON business responses

Use `ApiResponse` in a route when clients need a shared business response format. The envelope contains `success`, `code` and `message`. It includes `data`, pagination `meta` or field `errors` when supplied. `success(data)` defaults to HTTP 200. Pass a status to `failure` or set it with `with_status`.

```rust
use openrust::{
    http::{ApiResponse, PageMeta, StatusCode},
    routing::RoutingError,
    support::facades::Route,
};

fn register() -> Result<(), RoutingError> {
    Route::get("/items", || {
        ApiResponse::success([1, 2]).with_meta(PageMeta {
            page: 2,
            per_page: 2,
            total: 4,
            total_pages: 2,
        })
    })?;

    Route::get("/example-conflict", || {
        ApiResponse::failure(StatusCode::CONFLICT, "conflict", "The item already exists.")
    })
}
```

`ApiResponse::empty()` returns a success envelope without a `data` field. HTTP 204 responses have an empty body. Supply a client-safe message to `failure` and attach field messages with `with_errors` when needed.

A serialization error goes through [exception handling](errors.md). Built-in exceptions and the health endpoint each use their own response format. The business envelope is an OpenRust extension to [Laravel's JSON response mechanism](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#json-responses).

## Redirect responses

Use `Redirect::to` for a URL or `Redirect::route` for a named route. `RedirectOptions` defaults to 302 and accepts response headers. `Redirect::guest` stores the intended URL in the request session, and `Redirect::intended` consumes it and uses a fallback when the session has no intended URL.

Use `Redirect::action` for a [registered controller action](./urls.md#urls-for-controller-actions).

Use `Redirect::refresh` to redirect to the current path without its query, and `Redirect::secure` to generate an HTTPS destination from an application path. `Redirect::away` writes an explicit HTTP(S) URL or relative reference to `Location`. Supply a trusted destination. Use `Redirect::signedRoute` and `Redirect::temporarySignedRoute` to issue a redirect to an absolute signed URL. `Redirect::back` prefers a same-origin Referer, then the previous session URL, then the supplied fallback. `Redirect::getIntendedUrl` reads the current intended URL and `Redirect::setIntendedUrl` sets it explicitly. Both take the current request.

For a route-level redirect, `Router::redirect` and `Router::permanentRedirect` can substitute source parameters in the destination. A missing optional destination segment such as `/articles/{slug?}` is omitted. The source parameter names `destination` and `status` are reserved.

### Redirect URL fragments {#redirect-url-fragments}

Call `withFragment("details")` to replace the current fragment, or `withoutFragment()` to remove it. Both keep the URL path and query, and update the `Location` header and the HTML fallback together. Each returns a `Result` so an invalid redirect target can be handled.

### Redirecting with flashed session data {#redirecting-with-flashed-session-data}

In a web route with a session, chain `with` to flash a value, `withInput` to retain input, and `withErrors` to attach a named `MessageBag`. The Rust methods take the current mutable request. Return the `RedirectResponse` directly from the handler. Its status, headers and session updates flow through the usual HTTP response handling.

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
            .onlyInput(request, &["name"])
            .await
    })
}
```

Pass `None` to `withInput` to read form or JSON fields and query parameters from the request. Uploaded files are excluded. Use `onlyInput` or `exceptInput` to select fields by dot path. The next request can read old input with `request.old(Some("name"))` and flashed values from the session.

## Other Response Types {#other-response-types}

### View Responses {#view-responses}

Use `Response::view` to render a prepared view with a status and headers. Pass a view from `view("name")` or select the first available template with the `View::first` facade. Rendering preserves the current request context.

### JSONP Responses {#jsonp-responses}

Use `Response::jsonp(status, Some(callback), data)` to send a JavaScript callback with JSON data. A callback may use dotted names and literal bracket keys. Invalid callback names produce HTTP 400. Passing `None` returns an ordinary JSON response.

### Markdown Responses {#markdown-responses}

Use `Response::markdown(status, content)` to send Markdown source with `text/markdown; charset=utf-8`. Clients decide how to render it.

### Empty Responses {#empty-responses}

Use `Response::noContent()` for an empty HTTP 204 response. Use `Response::empty(status)` to select another status and `withHeaders` to attach headers.

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

Use `Response::streamJson` with `StreamedJson::object` to wrap a stream of rows in a JSON object. `StreamedJson::items` serializes each row as it arrives. Use `StreamedJson::value` for fixed values and `StreamedJson::array` for nested arrays. Use `StreamedJson::entries` to stream key-value pairs into a JSON object.

Streamed JSON and JSON event payloads use Laravel's default string escaping. SSE JSON preserves non-ASCII text apart from characters that require escaping.

```rust
use futures_util::stream;
use openrust::http::{HttpError, Response, StatusCode, StreamedJson};

let users = stream::iter([Ok::<_, HttpError>(1), Ok(2)]);
let data = StreamedJson::object([("users", StreamedJson::items(users))]);
let response = Response::streamJson(StatusCode::OK, data);
```

### Event Streams (SSE) {#event-streams}

`Response::eventStream` sends `text/event-stream` and disables cache and proxy buffering. `StreamedEvent::message` uses the `update` event. Use `StreamedEvent::new` for named text events and `StreamedEvent::json` for JSON payloads. The default last event carries `</stream>`. Use `Response::eventStreamWithEnd` to supply a final event or pass `None` to omit it.

Once the headers have been sent, a JSON or download source failure ends the body with a transport error. An SSE source failure in the application is reported through its exception handler. The SSE connection closes without a final event.

### Streamed Downloads {#streamed-downloads}

`Response::streamDownload` sends byte chunks as a download. With a filename, it builds `Content-Disposition` using the requested disposition and adds a UTF-8 filename when needed. With `None`, the supplied headers determine the disposition.

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

`hasMacro` checks the current factory. `mixin` takes explicitly listed `ResponseMacro` values and a replacement flag. `flushMacros` clears the current application's registry. Each application has its own factory.
