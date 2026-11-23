# Error Handling

## Propagating domain errors

Use `?` for fixed error conversions that need no additional context. Controllers and middleware returning `HttpError` can propagate configuration failures directly:

```rust
use openrust::{http::HttpError, support::facades::Config};

fn locale_form_field() -> Result<String, HttpError> {
    Ok(Config::string("app.locale_form_field")?)
}
```

Configuration, container, translation, view, database, logging, Redis, hashing, JSON serialization, I/O, and task join errors retain their original source at the HTTP boundary and become `Internal` errors by default. The existing exception pipeline handles reporting, debugging, and production redaction with status 500. The default database conversion does not turn `ModelNotFound` into 404; routes returning model results retain their existing classification.

Image domain errors and errors from `openrust-horizon`, `openrust-pulse`, and `openrust-pennant` also default to `Internal`. The domain source is retained; nested errors are not automatically unwrapped to change the status code.

Lower layers continue returning their own domain errors. Container resolution failures can propagate directly into the corresponding configuration, translation, view, database, cache, queue, event, and logging results. Database, cache, hashing, session, configuration, and container failures in authentication retain `AuthError::Backend`. General components should not depend on HTTP simply to support `?`.

The `?` operator converts into the current return error type; it does not chain multiple `From` implementations automatically. For return expressions, use `Ok(operation()?)`, or `map_err(Into::into)` where the return type determines the conversion. Give closures an explicit `Result` type when multiple error types make inference ambiguous.

Keep `map_err` when it adds a path, operation, request, or phase context, or performs status classification, recovery, retries, or redaction. Filesystem, validation, collection debugging, and authentication errors already have specialized HTTP conversions. Check their semantics before replacing an explicit `Internal` wrapper with `?`.

## Reporting Exceptions

Configure reporting in the `with_exceptions` closure in `bootstrap/app.rs`. Use `report::<E>` to observe an exception type or a matching source. Callbacks run in registration order. `ReportDecision::Stop` stops later callbacks and the default report.

Configure [when queued jobs should stop retrying](../digging-deeper/queues.md#stopping-retries-by-exception) through the same `with_exceptions` callback.

By default, reportable errors go through the application's configured log channel at `Level::Error`. HTTP reports include the status and omit source details from the default message. Artisan keeps its stderr message and exit status. Use `level::<E>` to choose a level and `context` to attach fields to the record. Public [Context](../digging-deeper/context.md) fields are written as separate metadata.

### Ignoring Exceptions

HTTP 400, 403, 404 and 405 are ignored by default. Use `dont_report::<E>` to ignore a matching type, `dont_report_when` for a predicate, and `stop_ignoring::<HttpError>` to allow reporting those HTTP errors. Ignoring affects reporting. Rendering still produces the response.

### Deduplicating Reported Exceptions

Enable `dont_report_duplicates` and wrap errors that have a shared identity in `SharedException`. Clones share the same identity. Use `Application::report` for an explicit report, or return the shared error as the source of `HttpError::Internal`. Independently constructed errors can report even when their messages are identical.

```rust
use openrust::{
    contracts::debug::SharedException,
    foundation::Application,
    log::Level,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .with_exceptions(|exceptions| {
            exceptions
                .level::<std::io::Error>(Level::Warning)
                .dont_report_duplicates();
        })
        .create()?;
    app.boot().await?;
    let error = SharedException::new(std::io::Error::other("Job failed"));
    app.report(&error).await?;
    app.report(&error.clone()).await?;
    Ok(())
}
```

## Throttling Reported Exceptions

Register a policy in `with_exceptions` with `throttle::<E>`. The first matching callback that returns a policy takes effect. Use `ReportThrottle::Lottery(Lottery::odds(1, 100)?)` for sampling, `ReportThrottle::Limit` for a fixed attempt budget, or `ReportThrottle::Unlimited` to report without throttling.

This example passes `configure` to `with_exceptions` and allows two I/O error reports per minute.

```rust
use openrust::{
    cache::rate_limiting::Limit,
    foundation::configuration::{Exceptions, ReportThrottle},
};

fn configure(exceptions: &mut Exceptions) {
    exceptions.throttle::<std::io::Error>(|_| {
        Some(ReportThrottle::Limit(Limit::per_minute(2).by("transport")))
    });
}
```

Without `by`, the budget is keyed by the registered Rust error type. Policies with the same explicit key share a budget in the configured limiter cache. A failed cache operation allows reporting to continue. Throttling happens before report callbacks and logging. HTTP status and body, and Artisan stderr and exit code, follow their usual handling.

`Application::report` is asynchronous so explicit reports can use the same cache-backed policy as HTTP and Artisan.

## Rendering Exceptions

Use `render::<E>` to return an HTTP response for an error and request. Returning `None` passes control to the next matching renderer, then to the default JSON or HTML renderer. Use `respond` to adjust the final exception response, whether custom or default. Both callbacks apply to HTTP only.

### Customizing the Exception Response

For default HTML responses, add a UTF-8 page such as `resources/views/errors/404.html` or `resources/views/errors/500.html`. Lookup uses the configured view paths. Other error statuses can fall back to `4xx.html` or `5xx.html` before using the built-in page. JSON responses use their own renderer.

```rust
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, http::Kernel},
    http::{Body, HttpError, Request, Response, StatusCode},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Application::configure(std::env::current_dir()?)
        .with_exceptions(|exceptions| {
            exceptions
                .render::<HttpError>(|error, request| {
                    if matches!(error, HttpError::NotFound) && request.uri().path() == "/example" {
                        Some(Response::new(StatusCode::NOT_FOUND, Body::from("Example not found")))
                    } else {
                        None
                    }
                })
                .respond(|mut response, _, _| {
                    response.parts_mut().headers.insert("x-error", "handled".parse().unwrap());
                    response
                });
        })
        .create()?;

    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await?;
    let (parts, ()) = http::Request::get("/example").body(())?.into_parts();
    let mut request = Request::from_parts(parts, Body::empty());
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()["x-error"], "handled");
    Ok(())
}
```

## Rendering Exceptions as JSON

The framework uses the request's JSON preference by default. The application skeleton configures `with_exceptions` to render JSON for the effective API namespace and for requests that expect JSON. Use `should_render_json_when` to set a different policy.

Set `APP_DEBUG=false` in production. The default JSON response uses `Server Error` for a generic 500, and the default HTML page omits exception sources. In debug mode, built-in server-error responses include a bounded list of source messages, escaped when displayed as HTML. Custom callbacks control the contents of responses they return.

For example, a missing API route returns `404` with a JSON `message` field. A missing Web route returns an HTML `404`, or JSON if the request prefers a JSON media type in its `Accept` header.

## Missing Routes

The application's `with_exceptions` policy returns JSON for the effective API namespace or requests expecting JSON. Other missing routes return HTML. This changes the response format, not the 404 status. The same policy applies to missing static files; HEAD preserves the status and content type with an empty body.

The path rule matches descendants of the effective prefix, such as `/v1/internal/users` for `/v1/internal`. With an empty prefix, the skeleton uses the request’s JSON preference.

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```

## HTTP Exceptions {#http-exceptions}

Return `Err(abort(status, message, headers))` from a route handler or middleware to stop processing and render an HTTP exception. The exception handler keeps the status and headers and negotiates HTML or JSON. Use a message intended for the client. An unexpected internal error still uses the configured production redaction policy.

```rust
use openrust::http::{HeaderMap, HttpError, Response, StatusCode, abort};

async fn maintenance() -> Result<Response, HttpError> {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", "60".parse().unwrap());
    Err(abort(StatusCode::SERVICE_UNAVAILABLE, "Maintenance", headers))
}
```

## Custom HTTP Error Pages

Add a status-specific view such as `resources/views/errors/404.blade.html`. Blade error views receive `$status` and `$message`. A plain `errors/404.html` also works. You can add `errors/4xx.blade.html` or `errors/5xx.blade.html` for a status family. Statuses 404, 500 and 503 use their dedicated view or the framework fallback. The original status and JSON negotiation apply.

## Exceptions facade {#exceptions-facade}

`Exceptions` resolves the current application’s `dyn ExceptionHandler` binding. HTTP middleware error boundaries, application reporting and Artisan failures resolve that same contract; replacing it also takes effect when a request replaces the handler before failing. Worker reporting already resolves the contract. A missing binding produces an error rather than silently reusing the original concrete handler.

```rust
use openrust::{support::facades::Exceptions, log::LogResult};

async fn report_failure() -> LogResult<()> {
    Exceptions::report(std::io::Error::other("报告失败")).await
}
```

`report` accepts an owned Rust error. `should_report`, `should_stop_retries`, `render` and `render_for_console` forward the existing contract, while `handler` returns its underlying handler Arc, unwrapping an exception fake. `should_report` checks duplicate/ignore rules and performs sampling or rate-limit admission. Repeated queries may consume quota. Each actual report checks admission once; do not use it as a side-effect-free status probe. Bootstrap configuration remains in `with_exceptions`. Runtime forwards now include `reportable`, `renderable`, `dont_report`, `ignore`, `stop_ignoring`, `dont_report_when`, `dont_retry`, `dont_retry_when`, `dont_flash`, `level`, `build_context_using`, `respond_using`, `should_render_json_when`, `dont_report_duplicates` and typed `throttle_using`. `is_reporting` checks whether that exact exception instance is currently being reported; it does not test report eligibility or consume a throttling admission. Custom handlers must explicitly support runtime configuration and this read-only query; otherwise these operations return an error. Exception fake and assertion APIs are described below.

### Reportable handlers {#reportable-handlers}

`reportable` returns a live `ReportableHandler`. Returning `ReportDecision::Stop` stops subsequent callbacks and default logging for that report. Calling `.stop()` sets the same behavior on the registered handler, including when called after registration. The handle and its clones share the flag. Use `reportable_all` for a callback that matches every error without a type-specific registration.

```rust
use openrust::{contracts::container::BindingResolutionError, foundation::configuration::ReportDecision, support::facades::Exceptions};

fn configure_reports() -> Result<(), BindingResolutionError> {
    Exceptions::reportable::<std::io::Error>(|_| ReportDecision::Continue)?.stop();
    Exceptions::reportable_all(|_| ReportDecision::Stop)?;
    Ok(())
}
```

`handles` checks a typed handler against an `Exception`; `invoke` returns an error for an incompatible type. Callback execution still occurs before the stop flag is applied. Existing bootstrap `exceptions.report(...)` keeps its fluent configuration return; use bootstrap `exceptions.reportable(...)` when a reportable handle is needed.

### Exception testing {#exception-testing}

Install `Exceptions::fake()` inside the current application facade scope, or use `TestApp::fake_exceptions(&[])` and retain the returned handle across HTTP and Artisan calls. An empty selection records all reportable errors. `ExceptionType::of::<E>()` selects the concrete root error; `including_sources::<E>()` explicitly selects by source chain. Typed assertions still check the root type, rather than counting a nested source as another reported exception.

```rust
use openrust::{http::HttpError, testing::TestApp};

async fn check_reports(app: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let exceptions = app.fake_exceptions(&[]).await?;
    let _response = app.get("/failure").await?;
    exceptions.assert_reported::<HttpError>();
    exceptions.assert_reported_where::<HttpError>(|error| error.to_string().contains("failure"));
    exceptions.assert_reported_count(1);
    Ok(())
}
```

`assert_reported`, `assert_not_reported`, their `_where` variants, `assert_reported_count`, and `assert_nothing_reported` are available on the handle and the facade. `reported()` returns shared original errors. Predicates run without holding the report-list lock. HTTP rendering, validation redirects and console rendering use the original handler; selected reports do not also reach its logger. Unselected errors delegate normally. `handler()` unwraps the current fake; installing another fake starts a fresh list without nesting the previous fake. `set_handler` replaces its delegate while retaining reports.

`Exceptions::report(error)` and `Application::report_owned(error)` take ownership. HTTP, Artisan and Worker boundaries retain shared errors too. When implementing a custom handler caller with an already borrowed opaque error, use `SharedException` before borrowing it; a fake cannot safely retain an arbitrary borrowed error and returns an explicit error instead of storing a string snapshot. Applications and concurrent tasks do not share fake records.

`throw_on_report()` interrupts a protected test invocation after recording the error. HTTP calls return the original error through their `Result`. `TestApp::try_artisan(args)` likewise returns `Result<TestCommand, SharedException>`, including errors reported by `queue:work`. The compatibility `artisan(args)` API still returns `TestCommand`; when explicit throw mode is active, it rethrows via a panic carrying the original `SharedException`. Prefer `try_artisan` for fallible Rust tests.

For another async entry point, wrap its future in `Exceptions::catch_reported(future).await`. Each capture has a lexical task-local channel: a matching report returns its original error and cancels that invocation before later code runs. Nested captures restore the outer scope; concurrent requests and cancellation do not retain a stale channel. Outside a capture, direct `report(...).await` returns the original error in `Err` rather than parking the task. `throw_first_reported()` returns the first recorded error, or succeeds for an empty list. This test control does not turn production logging failures into panics.

### Exception context {#exception-context}

`Exceptions::register()` forwards the actual handler's registration hook; the standard handler has no extra registration work. A custom handler can override it, and an exception fake forwards it to its original handler.

`exception_context::<E>(callback)` registers fields for a concrete Rust error. For an error with its own `context()` method, pass `|error| error.context()` explicitly: `std::error::Error` does not define that optional method. `build_context_using(callback)` remains the one-argument convenience API. `build_context_using_with_context(callback)` receives both the exception and fields accumulated so far; callbacks run in registration order and later keys replace earlier keys.

`build_context_for_exception(&exception)` returns exception-specific fields. `context_for_exception(&exception).await?` returns an `ExceptionContext`: `fields` contains the merged fields and `exception` retains the original borrowed error, rather than a JSON imitation of an error object. The default context merges the currently resolved default-guard user ID after the error fields, so its `userId` wins; false-like IDs are omitted. The HTTP error boundary resolves the default guard lazily before logging. Authentication failures leave the original exception report intact; reporting inside authentication does not recurse into the same application guard. Nested applications and cancelled invocations restore their own context.

The actual log handler uses the same context builder. `SharedException` transport wrappers do not appear as extra causes in JSON logs, and do not invoke the original error's context callback twice. Genuine source errors remain in the exception chain. Reading or reporting context in another application/request does not reuse a prior authenticated identity.

When a route already holds `&mut Request`, use `context_for_exception_with_request(request, &exception).await?` or `report_for_request(request, error).await`. Alternatively, lend the request with `request.with_container(...)` before using the ambient facade. This makes the mutable borrow explicit and permits lazy authentication without unsafe global request references. The argument-free context path resolves the ambient request when available; outside HTTP it omits request identity.

A custom handler that does not implement `register`, `build_context_for_exception` or `context_for_exception` returns an explicit unsupported-capability error, including through a fake. An empty context is reserved for a handler that actually implements the operation and has no fields.

### Exception mapping {#exception-mapping}

`Exceptions::map::<Source, Target>(callback)` registers a typed transformation. The equivalent startup configuration is `exceptions.map::<Source, Target>(callback)`. The first matching mapping wins; replacing the same source type preserves its position. A result is not recursively remapped. Rust types and a constructor callback replace dynamic class-name strings, so invalid source/target types are compile-time errors.

For Rust enums, `map_optional::<E, Target>(callback)` can select individual variants: return `Some(mapped)` for a match or `None` to leave an unmatched error unchanged and try later mappings. This avoids transforming every `HttpError` variant when only one status should change.

```rust
use openrust::{contracts::container::BindingResolutionError, support::facades::Exceptions};

fn configure_mapping() -> Result<(), BindingResolutionError> {
    Exceptions::map::<std::num::ParseIntError, _>(|error| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, error.clone())
    })?;
    Ok(())
}
```

Actual reporting maps before ignore rules, report callbacks and logging. Rendering performs its own mapping, so an HTTP failure may invoke the mapper once for reporting and once for rendering. `should_report` queries the supplied error without applying a map. A selected fake records the original error, while rendering still delegates to the original handler's mapping.

The mapped root controls report/render/ignore rules: a retained source error remains diagnostic data and does not accidentally select its old policy. A mapping to `HttpError` keeps its status and response handling. `should_render_json_when` now receives the actual `Exception`, including mapped domain errors; use `downcast_ref::<E>()` when the policy depends on its type. There is no synthetic HTTP exception in place of a business error.

For a wrapper with an optional inner exception, register `inner_exception::<Wrapper>(callback)` returning `Option<SharedException>`. This explicitly adapts an inner-error method that is not part of Rust's `Error` trait. A nonempty inner error takes priority over normal maps and is returned without remapping; `None` continues matching. Clone the shared inner handle to preserve its original instance.

### Disabling exception handling {#without-exception-handling}

Use `without_exception_handling(&[])` to return unhandled HTTP errors from test calls, and `with_exception_handling()` to restore the first original handler. Non-Clone errors retain their actual owned instance. A missing route includes the method and URL without query parameters, while its original `HttpError::NotFound` remains in the source chain.

```rust
use openrust::{contracts::debug::Exception, http::HttpError, testing::TestApp};

async fn check_unhandled(app: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.without_exception_handling(&[]).await?;
    let error = app.get("/missing").await.expect_err("expected an unhandled error");
    assert!(Exception::Error(error.as_ref()).downcast_chain_ref::<HttpError>().is_some());
    app.with_exception_handling().await?;
    Ok(())
}
```

`handle_exceptions(&types)` keeps rendering the selected types; `handle_validation_exceptions()` keeps the normal validation response. `ExceptionType::matching::<HttpError>(predicate)` selects individual Rust enum variants. Repeated mode changes do not nest old disabled handlers. A fake installed before or after disabling handling keeps its report list; it records otherwise ignored exceptions while handling is disabled, then respects the original reporting rules after restoration. Console failures still render normally. Other applications keep their own mode.

The public `Exceptions::render` returns a fallible boxed-error result, retaining the original shared error in disabled mode. Borrowed opaque errors cannot be retained: pass `SharedException`, use an owned HTTP test entry, or handle the distinct `ExceptionOwnershipError`. `SharedException::from_exception` clones an existing shared owner; `new` / `from_arc` preserve the identity when a shared error is supplied again, including a mapper returning the same target, so duplicate reporting remains correct.

`Exception::downcast_chain_ref::<E>()` explicitly inspects diagnostic sources, including a mapped error's previous exception; `including_sources` selectors use it too. Mapped rule matching still uses only the new root. Source searches stop after 32 nodes, so a cyclic `source()` does not hang a test or report.

`TestApp` captures unhandled errors automatically. For a low-level kernel future, use `Exceptions::catch_reported(future).await`; direct legacy handler methods returning only `Response` use a typed `SharedException` panic in explicitly disabled mode. Use the fallible facade or test entry points when a `Result` is needed.
