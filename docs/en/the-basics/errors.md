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

## Rendering Exceptions as JSON

The application declares its exception policy with `withExceptions` in `bootstrap/app.rs`. Paths matching `api/*` and requests that expect JSON receive a JSON exception response. Other requests receive an HTML error page.

For example, a missing API route returns `404` with a JSON `message` field. A missing Web route returns an HTML `404`, or JSON if the request prefers a JSON media type in its `Accept` header.

## Missing Routes

The application's `withExceptions` policy returns JSON for paths matching `api/*` or requests expecting JSON. Other missing routes return HTML. This changes the response format, not the 404 status. The same policy applies to missing static files; HEAD preserves the status and content type with an empty body.

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```
