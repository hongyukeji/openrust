# HTTP Client

## Making requests {#making-requests}

Use the application-scoped `Http` facade from `openrust::support::facades` in application handlers. `Http::get(url).await?` returns a response for every HTTP status. Inspect it with `status`, `successful`, or `failed`, and call `throw()` to turn a 4xx or 5xx into an error containing the response. Use `json::<T>()` for typed decoding or `body()` for the buffered bytes.

Use `Http::acceptJson()?` or `Http::pendingRequest()?` to start a configurable request. Supply POST, PUT, PATCH, or DELETE data by reference. JSON is the default. Use `asForm`, `withBody`, or `attach` to select another encoding. `withQueryParameters` sets the encoded query. Without it the query in the URL is preserved. Set timeouts with `Duration` and use `maxResponseBytes` to limit buffered bytes. An application shares the connection pool for default connection settings.

## Retries and testing {#retries-and-testing}

`retry` takes the total attempt count. HTTP failures return the last response as an error by default. `throwOnRetryFailure(false)` returns that response directly, while transport failures still return errors. Requests with non-replayable bodies require a fresh body for each separate send. `Factory::new` also works outside an application and owns its own fakes and recorded requests. Use `preventStrayRequests(true)` to make unmatched test requests fail before connecting.

```rust
use http::{HeaderMap, StatusCode};
use openrust::http::client::{Factory, Response};
use serde_json::{Value, json};
use std::time::Duration;

let factory = Factory::new()?;
factory.fakeSequence("example.test/catalog")
    .push(Ok(Response::new(StatusCode::SERVICE_UNAVAILABLE, HeaderMap::new(), Vec::new())))
    .push(Response::from_json(StatusCode::OK, &json!({"items": ["rust"]})));
factory.preventStrayRequests(true);

let response = factory.pendingRequest()
    .acceptJson()
    .retry(2, Duration::ZERO)
    .get("https://example.test/catalog")
    .await?
    .throw()?;
assert_eq!(response.json::<Value>()?["items"][0], "rust");
factory.assertSentCount(2);
```

Use `pool` with a nonzero concurrency limit and `Pool::r#as` to register named futures. Results preserve registration order and keep each request’s success or error.
