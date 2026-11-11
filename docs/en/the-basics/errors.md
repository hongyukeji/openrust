# Error Handling

## Reporting Exceptions

Configure reporting in the `withExceptions` closure in `bootstrap/app.rs`. Use `report::<E>` to observe an exception type or a matching source. Callbacks run in registration order. `ReportDecision::Stop` stops later callbacks and the default report.

By default, reportable errors go through the application's configured log channel at `Level::Error`. HTTP reports include the status and omit source details from the default message. Artisan keeps its stderr message and exit status. Use `level::<E>` to choose a level and `context` to attach fields to the record. Public [Context](../digging-deeper/context.md) fields are written as separate metadata.

### Ignoring Exceptions

HTTP 400, 403, 404 and 405 are ignored by default. Use `dontReport::<E>` to ignore a matching type, `dontReportWhen` for a predicate, and `stopIgnoring::<HttpError>` to allow reporting those HTTP errors. Ignoring affects reporting. Rendering still produces the response.

### Deduplicating Reported Exceptions

Enable `dontReportDuplicates` and wrap errors that have a shared identity in `SharedException`. Clones share the same identity. Use `Application::report` for an explicit report, or return the shared error as the source of `HttpError::Internal`. Independently constructed errors can report even when their messages are identical.

```rust
use openrust::{
    contracts::debug::SharedException,
    foundation::Application,
    log::Level,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withExceptions(|exceptions| {
            exceptions
                .level::<std::io::Error>(Level::Warning)
                .dontReportDuplicates();
        })
        .create()?;
    app.boot().await?;
    let error = SharedException::new(std::io::Error::other("Job failed"));
    app.report(&error)?;
    app.report(&error.clone())?;
    Ok(())
}
```

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
        .withExceptions(|exceptions| {
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

The framework uses the request's JSON preference by default. The application skeleton configures `withExceptions` to render JSON for `api/*` and for requests that expect JSON. Use `shouldRenderJsonWhen` to set a different policy.

Set `APP_DEBUG=false` in production. The default JSON response uses `Server Error` for a generic 500, and the default HTML page omits exception sources. In debug mode, built-in server-error responses include a bounded list of source messages, escaped when displayed as HTML. Custom callbacks control the contents of responses they return.

For example, a missing API route returns `404` with a JSON `message` field. A missing Web route returns an HTML `404`, or JSON if the request prefers a JSON media type in its `Accept` header.

## Missing Routes

The application's `withExceptions` policy returns JSON for paths matching `api/*` or requests expecting JSON. Other missing routes return HTML. This changes the response format, not the 404 status. The same policy applies to missing static files; HEAD preserves the status and content type with an empty body.

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```
