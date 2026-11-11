# HTTP Responses

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
