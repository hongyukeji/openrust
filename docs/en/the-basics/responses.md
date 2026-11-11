# HTTP Responses

## Creating Responses

Use `Response::json` for a JSON payload and an explicit status. Use `header` or `withHeaders` with typed HTTP header values. Use `Response::stream` to send chunks from a Rust stream.

See the [controller example](./controllers.md) for input handling and a JSON response.

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
