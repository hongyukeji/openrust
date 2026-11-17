# HTTP Responses

## Creating Responses

Use `Response::json` for a JSON payload and an explicit status. Use `header` or `withHeaders` with typed HTTP header values. Use `Response::stream` to send chunks from a Rust stream.

See the [controller example](./controllers.md) for input handling and a JSON response.

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

### Redirecting with flashed session data {#redirecting-with-flashed-session-data}

In a web route with a session, chain `with` to flash a value, `withInput` to retain input, and `withErrors` to attach a named `MessageBag`. The Rust methods take the current mutable request. Convert the redirect to an HTTP response when returning it from the handler.

```rust
use openrust::{
    http::{HttpError, Request, Response},
    support::facades::Redirect,
};
use serde_json::json;

async fn save_profile(request: &mut Request) -> Result<Response, HttpError> {
    let redirect = Redirect::to("/profile", Default::default())?
        .with(request, "status", json!("saved"))?
        .onlyInput(request, &["name"])
        .await?;
    Ok(redirect.into())
}
```

Pass `None` to `withInput` to read form or JSON fields and query parameters from the request. Uploaded files are excluded. Use `onlyInput` or `exceptInput` to select fields by dot path. The next request can read old input with `request.old(Some("name"))` and flashed values from the session.

## Streamed Responses {#streamed-responses}

### Streamed JSON Responses {#streamed-json-responses}

Use `Response::streamJson` with `StreamedJson::object` to wrap a stream of rows in a JSON object. `StreamedJson::items` serializes each row as it arrives. Use `StreamedJson::value` for fixed values and `StreamedJson::array` for nested arrays. Use `StreamedJson::entries` to stream key-value pairs into a JSON object.

```rust
use futures_util::stream;
use openrust::http::{HttpError, Response, StatusCode, StreamedJson};

let users = stream::iter([Ok::<_, HttpError>(1), Ok(2)]);
let data = StreamedJson::object([("users", StreamedJson::items(users))]);
let response = Response::streamJson(StatusCode::OK, data);
```

### Event Streams (SSE) {#event-streams}

`Response::eventStream` sends `text/event-stream` and disables cache and proxy buffering. `StreamedEvent::message` uses the `update` event. Use `StreamedEvent::new` for named text events and `StreamedEvent::json` for JSON payloads. The default last event carries `</stream>`. Use `Response::eventStreamWithEnd` to supply a final event or pass `None` to omit it.

Once the headers have been sent, a source or serialization error ends the body with a transport error. SSE sends its final event only when the source completes successfully.

### Streamed Downloads {#streamed-downloads}

`Response::streamDownload` sends byte chunks as a download. With a filename, it builds `Content-Disposition` using the requested disposition and adds a UTF-8 filename when needed. With `None`, the supplied headers determine the disposition.
