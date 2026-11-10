# Error Handling

## Reporting and rendering exceptions

Register callbacks in the `withExceptions` closure in `bootstrap/app.rs`. Use `report::<E>` for an error type `E`. Matching includes the error's source chain. Reporters run in registration order. Return `ReportDecision::Continue` to continue reporting or `ReportDecision::Stop` to stop later reporters and default reporting. Normal HTTP 400, 403, 404 and 405 responses are excluded from reporting. Unhandled HTTP 500s write a diagnostic to stderr. Artisan reports command execution and boot errors and keeps its default stderr rendering and exit status.

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
