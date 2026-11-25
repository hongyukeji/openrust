# HTTP Client

## Making requests {#making-requests}

Use the application-scoped `Http` facade from `openrust::support::facades` in application handlers. `Http::get(url).await?` returns a response for every HTTP status. Inspect it with `status`, `successful`, or `failed`, and call `throw()` to turn a 4xx or 5xx into an error containing the response. Use `json::<T>()` for typed decoding or `body()?` to read the body as `Bytes` on demand. File read failures return `Error::ResponseBody`.

Use `Http::accept_json()?` or `Http::pending_request()?` to start a configurable request. Supply POST, PUT, PATCH, or DELETE data by reference. JSON is the default. Use `as_form`, `with_body`, or `attach` to select another encoding. `with_query_parameters` sets the encoded query. Without it the query in the URL is preserved. Set timeouts with `Duration` and use `max_response_bytes` to limit buffered bytes. An application shares the connection pool for default connection settings.

Request and response callbacks use Rust types. Select transport settings through the request builder and use Rust functions or extension traits to reuse named configurations.

### Digest authentication {#digest-authentication}

Use `with_basic_auth` for Basic credentials and `with_token` for Bearer tokens. Use `with_digest_auth` to answer a Digest challenge from the original origin. Digest replays buffered requests, and can retry once when a server reports a stale nonce. Only the terminal response reaches `after_response`. The example uses a local `GET /digest` endpoint configured with the sample credentials and an `authenticated` response body.

```rust
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let response = Http::with_digest_auth("example-user", "example-password")?
    .get(&format!("{origin}/digest"))
    .await?
    .throw()?;
assert_eq!(response.body()?.as_ref(), b"authenticated");
```

## Retries and testing {#retries-and-testing}

`retry` takes the total attempt count. HTTP failures return the last response as an error by default. `throw_on_retry_failure(false)` returns that response directly, while transport failures still return errors. Requests with non-replayable bodies require a fresh body for each separate send. `Factory::new` also works outside an application and owns its own fakes and recorded requests. Use `prevent_stray_requests(true)` to make unmatched test requests fail before connecting.

```rust
use http::{HeaderMap, StatusCode};
use openrust::http::client::{Factory, Response};
use serde_json::{Value, json};
use std::time::Duration;

let factory = Factory::new()?;
factory.fake_sequence("example.test/catalog")
    .push(Ok(Response::new(StatusCode::SERVICE_UNAVAILABLE, HeaderMap::new(), Vec::new())))
    .push(Response::from_json(StatusCode::OK, &json!({"items": ["rust"]})));
factory.prevent_stray_requests(true);

let response = factory.pending_request()
    .accept_json()
    .retry(2, Duration::ZERO)
    .get("https://example.test/catalog")
    .await?
    .throw()?;
assert_eq!(response.json::<Value>()?["items"][0], "rust");
factory.assert_sent_count(2);
```

Use `pool` with a nonzero concurrency limit. Use `NonZeroUsize::MAX` to start all requests without a cap. Results are keyed in completion order. `pool_ordered` starts all requests and returns the same per-request results in registration order. Use `r#as` to name a request or `new_request` to assign the next numeric key as a string.

### URI templates, retry callbacks, and URL allowances {#uri-templates-retry-and-allowance}

Use `with_url_parameters` to expand RFC 6570 expressions before sending. Supply strings, numbers, booleans, lists, or associative objects in a JSON map. A null value omits its variable. Exploded query objects expand nested fields using bracketed keys. Prefix lengths count Unicode characters. Later calls replace values with the same name. A URL with literal braces can be sent without specifying any template parameters.

Call `retry_when_request` to inspect the failure and update the pending replay, such as replacing an authorization header after a 401. Return `Ok(true)` to retry, `Ok(false)` to stop, or an error to abort. Changes persist in later retries.

When `prevent_stray_requests(true)` is active, use `allow_stray_requests` to replace the list of URL patterns that may reach the network. Fakes take precedence and redirects are checked at each hop.

Set `HTTP_CLIENT_EXAMPLE_ORIGIN` to a local service that returns 401 for the example URL until it receives the `x-replay: ready` header, then returns `retried`.

```rust
use openrust::{http::client::Error, support::facades::Http};
use serde_json::json;
use std::time::Duration;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
Http::prevent_stray_requests(true)?;
Http::allow_stray_requests([format!("{origin}/retry/*")])?;

let parameters = [
    ("origin".to_owned(), json!(origin)),
    ("item".to_owned(), json!("item/id")),
    ("tag".to_owned(), json!("x y")),
].into_iter().collect();

let response = Http::with_url_parameters(parameters)?
    .retry(2, Duration::ZERO)
    .retry_when_request(|error, next| {
        if matches!(error, Error::RequestFailed(response) if response.unauthorized()) {
            next.headers_mut().insert("x-replay", "ready".parse().expect("valid header"));
            return Ok(true);
        }
        Ok(false)
    })
    .get("{+origin}/retry/{item}{?tag}")
    .await?;
assert_eq!(response.body()?.as_ref(), b"retried");
assert!(matches!(Http::get(&format!("{origin}/blocked")).await, Err(Error::StrayRequest)));
```

## Batches and events {#batches-and-events}

Build a batch with `Http::batch` and execute it with `send().await`. Set `concurrency` to cap the number of active requests. `before` runs before the first request, `progress` receives 2xx responses, and `catch` receives 4xx and 5xx responses or per-request errors. A 3xx response only advances the completed count. `then` runs when there are no failures, and `finally` always receives results in registration order.

Use `throw()` or `throw_if` on a pending request to convert a matching final 4xx or 5xx response into a per-request error. `defer()?` consumes a batch with owned inputs and schedules it for termination of the current HTTP request, Artisan command, or queue job. By default it runs after a successful task. Call `always()` on the deferred callback to run it after a failed task as well.

Set `HTTP_CLIENT_EXAMPLE_ORIGIN` to a local service with `GET /ok` returning 200 and `GET /missing` returning 404 to run this example.

```rust
use std::num::NonZeroUsize;
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let ok = format!("{origin}/ok");
let missing = format!("{origin}/missing");
let pool = Http::pool(NonZeroUsize::MIN, |pool| {
    pool.new_request(|request| request.get(&ok))
        .r#as("missing", |request| request.get(&missing));
}).await?;
assert!(pool["0"].as_ref().is_ok_and(|response| response.ok()));
assert!(pool["missing"].as_ref().is_ok_and(|response| response.not_found()));

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

Register listeners for `RequestSending`, `ResponseReceived`, and `ConnectionFailed` from `openrust::http::client::events`. An application factory uses its application’s dispatcher. For a standalone factory, supply one with `with_dispatcher`. Use `with_attributes` for local metadata that listeners can read from the request snapshot. It stays in the client.

`RequestSending` fires before per-request `before_sending` callbacks on each outgoing hop. `ResponseReceived` has the terminal response and fires before `after_response`, including for HTTP 4xx and 5xx. `ConnectionFailed` has the attempted request and a typed connection or timeout error. Retries repeat the event sequence. A request rejected by stray-request prevention emits only `RequestSending`.

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

assert!(Http::get(&format!("{origin}/redirect")).await?.not_found());
assert!(matches!(Http::get(&unavailable).await, Err(Error::Connection)));
let outgoing = events.dispatched(sending);
assert_eq!(outgoing.len(), 3);
assert_eq!(outgoing[1].downcast_ref::<RequestSending>().unwrap().request.url.path(), "/missing");
let replies = events.dispatched(received);
assert_eq!(replies.len(), 1);
assert!(replies[0].downcast_ref::<ResponseReceived>().unwrap().response.not_found());
let failures = events.dispatched(failed);
assert_eq!(failures.len(), 1);
assert_eq!(failures[0].downcast_ref::<ConnectionFailed>().unwrap().request.url.as_str(), unavailable);
```

## Global middleware and transfer timing {#global-middleware-and-timing}

Register `Http::global_middleware` in your application provider to wrap each outgoing hop; `Factory::global_middleware` also supports standalone clients. A wrapper receives the owned request and a continuation `next`, and returns `Box::pin(async move { ... })`. `next.run(request).await` returns the response or transport error, including timeout. Wrappers run in registration order and unwind in reverse; redirects and retries re-enter them. Registration applies to subsequently created pending requests, is shared by clones of the same factory, and stays isolated between applications.

`global_request_middleware` edits the request through `&mut Request` and returns `Result<()>`; `global_response_middleware` maps a `Response` to `Result<Response>`. For a single pending request use `with_middleware`, `with_request_middleware`, or `with_response_middleware`. Existing `before_sending` and `after_response` callbacks remain available.

```rust
use openrust::http::client::Http;

Http::global_middleware(|request, next| Box::pin(async move {
    let response = next.run(request).await?;
    let seconds = response.handler_stats().get("total_time").copied();
    // Use `seconds` with the application's metrics collector.
    Ok(response)
}))?;
```

`Response::handler_stats()` reports `total_time` in seconds for the final network hop, including buffered response-body reads. It excludes retry delays and is not a sum of redirects or attempts. `response.transfer_stats.as_ref().map(|stats| stats.get_transfer_time())` exposes the same duration. Fake responses have zero transfer time and no network handler stats; a middleware-generated response has no transport stats unless a transport ran. The reqwest transport does not expose cURL phase breakdowns, so DNS, connection, and TLS timings are not fabricated. Redacted response events retain timing without exposing bodies or credentials.

## Request options

`replace_headers` replaces only the supplied header names, preserving other headers and multiple values. `with_header`, `content_type`, `as_multipart`, and `max_redirects` use the existing request builder.

The client follows 3xx responses with a valid HTTP(S) `Location`. For 300–303 it drops the body, keeps GET, HEAD, and OPTIONS, and switches other methods to GET. For 304–399 it preserves the method and requires a replayable body. Each redirect clears the previous `Referer` header.

`sink(path)` writes network chunks directly to a file and keeps a file handle in the response instead of buffering the full body. Pass an owned `std::fs::File` with `sink(file)` to write to a file resource. PendingRequest, Factory and the Http Facade provide this method. A path is created or truncated on its first write; a file resource is written from its current position without truncation, then rewound on completion. A successful empty response also creates or truncates a path. Each retry and redirect handles the path again. A connection failure before any body arrives preserves the old file.

`response.resource()?` implements `Read`, `Write` and `Seek`, sharing the body and cursor with response clones. Rust resources operate directly on that cursor rather than reproducing PHP wrapper read buffering. `body()?` reads the entire body from the start on each call; `json::<T>()?` also needs the complete body. These calls and body-reading observers such as Telescope still allocate body memory. `close()?` closes the shared body; subsequent reads, writes and seeks fail.

Downloads remain subject to `max_response_bytes` and timeouts. Write failures return `Error::Sink`. Disconnects, timeouts, cancellation and limit failures leave bytes already written in the file. Downloads are not atomic replacements and partial files are not deleted automatically. Fakes write their existing buffered body to the target and retain that body in the response.

`with_user_agent("OpenRust/1.0")` sets `User-Agent`, replaces an existing value, and trims leading and trailing ASCII space, tab, newline, carriage return, NUL and vertical tab. Unicode spaces remain unchanged. The argument is a Rust string; invalid header values return `InvalidRequest` before transmission. PendingRequest, Factory and the Http Facade all provide this method.

```rust
let response = Http::with_user_agent("OpenRust/1.0")?.get(url).await?;
let response = Http::without_verifying()?.get(test_https_url).await?;
```

TLS verification is enabled by default. `without_verifying()` disables certificate-chain and hostname checks only for the request being built; it does not change the shared Factory or later default requests. It also works with `connect_timeout`. Use it for controlled endpoints whose untrusted certificates you explicitly accept. The transport does not implement an NTLM authentication handshake and does not provide `withNtlmAuth`; Basic, Digest or a manually assembled Authorization header cannot substitute for that handshake.

## Multipart Field Values

`as_multipart().post(data)` accepts an object of scalar fields. Booleans use PHP-style `1`/empty-string encoding and null becomes an empty string. Nested objects and arrays return `InvalidRequest`; use `attach` for files. This entry does not accept every Guzzle multipart option.
