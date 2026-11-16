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

Use `pool` with a nonzero concurrency limit. Use `NonZeroUsize::MAX` to start all requests without a cap. Results are keyed in completion order. `poolOrdered` starts all requests and returns the same per-request results in registration order. Use `r#as` to name a request or `newRequest` to assign the next numeric key as a string.

## Batches and events {#batches-and-events}

Build a batch with `Http::batch` and execute it with `send().await`. Set `concurrency` to cap the number of active requests. `before` runs before the first request, `progress` receives 2xx responses, and `catch` receives 4xx and 5xx responses or per-request errors. A 3xx response only advances the completed count. `then` runs when there are no failures, and `finally` always receives results in registration order.

Use `throw()` or `throwIf` on a pending request to convert a matching final 4xx or 5xx response into a per-request error. `defer()?` consumes a batch with owned inputs and schedules it for termination of the current HTTP request, Artisan command, or queue job. By default it runs after a successful task. Call `always()` on the deferred callback to run it after a failed task as well.

Set `HTTP_CLIENT_EXAMPLE_ORIGIN` to a local service with `GET /ok` returning 200 and `GET /missing` returning 404 to run this example.

```rust
use std::num::NonZeroUsize;
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let ok = format!("{origin}/ok");
let missing = format!("{origin}/missing");
let pool = Http::pool(NonZeroUsize::MIN, |pool| {
    pool.newRequest(|request| request.get(&ok))
        .r#as("missing", |request| request.get(&missing));
}).await?;
assert!(pool["0"].as_ref().is_ok_and(|response| response.ok()));
assert!(pool["missing"].as_ref().is_ok_and(|response| response.notFound()));

let batch = Http::batch(|batch| {
    batch
        .r#as("ok", |request| request.get(&ok))
        .r#as("missing", |request| request.throw().get(&missing))
        .finally(|counts, results| {
            assert_eq!(counts.failed_requests, 1);
            assert_eq!(results.keys().map(String::as_str).collect::<Vec<_>>(), ["ok", "missing"]);
        });
})?;
let results = batch.send().await;
assert!(results["missing"].is_err());
```

Register listeners for `RequestSending`, `ResponseReceived`, and `ConnectionFailed` from `openrust::http::client::events`. Their event names use the `OpenRust\Http\Client\Events\` prefix. An application HTTP client dispatches events for each request attempt, including retries.
