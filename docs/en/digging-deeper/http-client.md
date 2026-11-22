# HTTP Client

## Making requests {#making-requests}

Use the application-scoped `Http` facade from `openrust::support::facades` in application handlers. `Http::get(url).await?` returns a response for every HTTP status. Inspect it with `status`, `successful`, or `failed`, and call `throw()` to turn a 4xx or 5xx into an error containing the response. Use `json::<T>()` for typed decoding or `body()` for the buffered bytes.

Use `Http::acceptJson()?` or `Http::pendingRequest()?` to start a configurable request. Supply POST, PUT, PATCH, or DELETE data by reference. JSON is the default. Use `asForm`, `withBody`, or `attach` to select another encoding. `withQueryParameters` sets the encoded query. Without it the query in the URL is preserved. Set timeouts with `Duration` and use `maxResponseBytes` to limit buffered bytes. An application shares the connection pool for default connection settings.

Request and response callbacks use Rust types. Select transport settings through the request builder and use Rust functions or extension traits to reuse named configurations.

### Digest authentication {#digest-authentication}

Use `withBasicAuth` for Basic credentials and `withToken` for Bearer tokens. Use `withDigestAuth` to answer a Digest challenge from the original origin. Digest replays buffered requests, and can retry once when a server reports a stale nonce. Only the terminal response reaches `afterResponse`. The example uses a local `GET /digest` endpoint configured with the sample credentials and an `authenticated` response body.

```rust
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let response = Http::withDigestAuth("example-user", "example-password")?
    .get(&format!("{origin}/digest"))
    .await?
    .throw()?;
assert_eq!(response.body(), b"authenticated");
```

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

### URI templates, retry callbacks, and URL allowances {#uri-templates-retry-and-allowance}

Use `withUrlParameters` to expand RFC 6570 expressions before sending. Supply strings, numbers, booleans, lists, or associative objects in a JSON map. A null value omits its variable. Exploded query objects expand nested fields using bracketed keys. Prefix lengths count Unicode characters. Later calls replace values with the same name. A URL with literal braces can be sent without specifying any template parameters.

Call `retryWhenRequest` to inspect the failure and update the pending replay, such as replacing an authorization header after a 401. Return `Ok(true)` to retry, `Ok(false)` to stop, or an error to abort. Changes persist in later retries.

When `preventStrayRequests(true)` is active, use `allowStrayRequests` to replace the list of URL patterns that may reach the network. Fakes take precedence and redirects are checked at each hop.

Set `HTTP_CLIENT_EXAMPLE_ORIGIN` to a local service that returns 401 for the example URL until it receives the `x-replay: ready` header, then returns `retried`.

```rust
use openrust::{http::client::Error, support::facades::Http};
use serde_json::json;
use std::time::Duration;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
Http::preventStrayRequests(true)?;
Http::allowStrayRequests([format!("{origin}/retry/*")])?;

let parameters = [
    ("origin".to_owned(), json!(origin)),
    ("item".to_owned(), json!("item/id")),
    ("tag".to_owned(), json!("x y")),
].into_iter().collect();

let response = Http::withUrlParameters(parameters)?
    .retry(2, Duration::ZERO)
    .retryWhenRequest(|error, next| {
        if matches!(error, Error::RequestFailed(response) if response.unauthorized()) {
            next.headers_mut().insert("x-replay", "ready".parse().expect("valid header"));
            return Ok(true);
        }
        Ok(false)
    })
    .get("{+origin}/retry/{item}{?tag}")
    .await?;
assert_eq!(response.body(), b"retried");
assert!(matches!(Http::get(&format!("{origin}/blocked")).await, Err(Error::StrayRequest)));
```

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

### Outgoing request events {#outgoing-request-events}

Register listeners for `RequestSending`, `ResponseReceived`, and `ConnectionFailed` from `openrust::http::client::events`. An application factory uses its application’s dispatcher. For a standalone factory, supply one with `withDispatcher`. Use `withAttributes` for local metadata that listeners can read from the request snapshot. It stays in the client.

`RequestSending` fires before per-request `beforeSending` callbacks on each outgoing hop. `ResponseReceived` has the terminal response and fires before `afterResponse`, including for HTTP 4xx and 5xx. `ConnectionFailed` has the attempted request and a typed connection or timeout error. Retries repeat the event sequence. A request rejected by stray-request prevention emits only `RequestSending`.

For this example, add `GET /redirect` to the local service with a 302 redirect to `/missing`. Set `HTTP_CLIENT_EXAMPLE_FAILURE` to a URL at a local endpoint that refuses the connection.

```rust
use openrust::http::client::{Error, events::{ConnectionFailed, RequestSending, ResponseReceived}};
use openrust::support::facades::{Event, Http};

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let unavailable = std::env::var("HTTP_CLIENT_EXAMPLE_FAILURE")?;
let sending = r"OpenRust\Http\Client\Events\RequestSending";
let received = r"OpenRust\Http\Client\Events\ResponseReceived";
let failed = r"OpenRust\Http\Client\Events\ConnectionFailed";
let events = Event::fake(&[sending, received, failed])?;

assert!(Http::get(&format!("{origin}/redirect")).await?.notFound());
assert!(matches!(Http::get(&unavailable).await, Err(Error::Connection)));
let outgoing = events.dispatched(sending);
assert_eq!(outgoing.len(), 3);
assert_eq!(outgoing[1].downcast_ref::<RequestSending>().unwrap().request.url.path(), "/missing");
let replies = events.dispatched(received);
assert_eq!(replies.len(), 1);
assert!(replies[0].downcast_ref::<ResponseReceived>().unwrap().response.notFound());
let failures = events.dispatched(failed);
assert_eq!(failures.len(), 1);
assert_eq!(failures[0].downcast_ref::<ConnectionFailed>().unwrap().request.url.as_str(), unavailable);
```

## Global middleware and transfer timing {#global-middleware-and-timing}

Register `Http::globalMiddleware` in your application provider to wrap each outgoing hop; `Factory::globalMiddleware` also supports standalone clients. A wrapper receives the owned request and a continuation `next`, and returns `Box::pin(async move { ... })`. `next.run(request).await` returns the response or transport error, including timeout. Wrappers run in registration order and unwind in reverse; redirects and retries re-enter them. Registration applies to subsequently created pending requests, is shared by clones of the same factory, and stays isolated between applications.

`globalRequestMiddleware` edits the request through `&mut Request` and returns `Result<()>`; `globalResponseMiddleware` maps a `Response` to `Result<Response>`. For a single pending request use `withMiddleware`, `withRequestMiddleware`, or `withResponseMiddleware`. Existing `beforeSending` and `afterResponse` callbacks remain available.

```rust
use openrust::http::client::Http;

Http::globalMiddleware(|request, next| Box::pin(async move {
    let response = next.run(request).await?;
    let seconds = response.handlerStats().get("total_time").copied();
    // Use `seconds` with the application's metrics collector.
    Ok(response)
}))?;
```

`Response::handlerStats()` reports `total_time` in seconds for the final network hop, including buffered response-body reads. It excludes retry delays and is not a sum of redirects or attempts. `response.transferStats.as_ref().map(|stats| stats.getTransferTime())` exposes the same duration. Fake responses have zero transfer time and no network handler stats; a middleware-generated response has no transport stats unless a transport ran. The reqwest transport does not expose cURL phase breakdowns, so DNS, connection, and TLS timings are not fabricated. Redacted response events retain timing without exposing bodies or credentials.

## Request options

`replaceHeaders` replaces only the supplied header names, preserving other headers and multiple values. `withHeader`, `contentType`, `asMultipart`, and `maxRedirects` use the existing request builder. `sink(path)` writes the successfully read response body to a file and reports write failures as `Error::Sink`. It currently buffers the body and respects `body_limit`; it is not an unbounded streaming download.

`withUserAgent("OpenRust/1.0")` sets `User-Agent`, replaces an existing value, and trims leading and trailing ASCII space, tab, newline, carriage return, NUL and vertical tab. Unicode spaces remain unchanged. The argument is a Rust string; invalid header values return `InvalidRequest` before transmission. PendingRequest, Factory and the Http Facade all provide this method.

```rust
let response = Http::withUserAgent("OpenRust/1.0")?.get(url).await?;
let response = Http::withoutVerifying()?.get(test_https_url).await?;
```

TLS verification is enabled by default. `withoutVerifying()` disables certificate-chain and hostname checks only for the request being built; it does not change the shared Factory or later default requests. It also works with `connectTimeout`. Use it for controlled endpoints whose untrusted certificates you explicitly accept. The transport does not implement an NTLM authentication handshake and does not provide `withNtlmAuth`; Basic, Digest or a manually assembled Authorization header cannot substitute for that handshake.

## Multipart Field Values

`asMultipart().post(data)` accepts an object of scalar fields. Booleans use PHP-style `1`/empty-string encoding and null becomes an empty string. Nested objects and arrays return `InvalidRequest`; use `attach` for files. This entry does not accept every Guzzle multipart option.
