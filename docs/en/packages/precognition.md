# Precognition

Precognition runs a form request's authorization and validation before a user submits the form. Successful validation returns an empty `204` response with `Precognition-Success: true`; invalid JSON requests return `422` with the usual `message` and `errors` fields.

## Enabling Precognition

Apply the built-in `precognitive` middleware to the routes you want to validate. Use an injected `ValidatedFormRequest` or `ControllerDispatcher::with_form_request` so validation runs before the controller action.

```rust
use openrust::{
    foundation::http::ValidatedFormRequest,
    http::{HttpError, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::Route,
};

async fn store(form: ValidatedFormRequest<StoreUser>) -> Result<Response, HttpError> {
    Response::json(StatusCode::OK, form.validated().all())
}

fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().middleware_name("precognitive"), |router| {
        router.post("/users", store)?;
        Ok(())
    })
}
```

`StoreUser` implements [FormRequest](../the-basics/validation.md). A request becomes precognitive only when the middleware confirms the exact header `Precognition: true`. `Request::is_attempting_precognition()` checks the header; `Request::is_precognitive()` checks the server's confirmation. Sending the header to a route without the middleware leaves ordinary execution and Session persistence intact.

## Selecting validation rules

Send `Precognition-Validate-Only: name,email` to validate selected fields. Wildcards select expanded attributes: `users.*.email` matches `users.0.email`, while `users.*` does not match the extra `.email` segment. Comma-separated names are not trimmed. An empty header selects no rules. Without the header, all rules run.

The middleware adds `Precognition: true` to attempted requests and appends `Precognition` to `Vary` for every response, including ordinary requests and validation or authorization failures. Middleware that returns an early response keeps its original status; it does not become a successful validation response.

## Request lifecycle and side effects

Middleware, controller construction, form construction, input preparation and authorization still run. A successful injected form request stops before the action executes. With `Precognition-Validate-Only`, the success hook also stops before `passed_validation`; without that header, `passed_validation` runs before dispatch stops. Keep these hooks and dependency factories free of unintended writes.

Confirmed requests start and read the normal Session and return its cookie, but skip saving it and updating the previous URL. This preserves flash data and prevents ordinary middleware Session changes from being persisted. Precognition does not roll back writes made by middleware, dependency constructors, custom validation rules, external services or explicit Session operations.

## Files and dynamic rules

The server validates uploaded files using the normal [file rules](../the-basics/validation.md). Frontend helpers omit files by default. Call `validateFiles()` when live validation should upload them. A required file omitted by the client still fails `required` unless you adjust the rules.

Override `FormRequest::validation_rules(&self, request: &Request)` to select rules based on `request.is_precognitive()`. Its default delegates to `rules()`. For example, require a password or file only for the final submission while keeping type and size checks on files that are sent.

## JavaScript helper

Use the official `laravel-precognition` core helper `2.0.0` with the configured OpenRust routes:

```js
import { client } from 'laravel-precognition';

await client.post('/api/users', { name: 'Ada' }, {
    only: ['name'],
    onPrecognitionSuccess: () => { /* valid */ },
    onValidationError: response => { console.log(response.data.errors); },
});

await client.post('/api/users', values, { precognitive: false });
```

The final submission executes the action normally. For Web routes, keep Session and CSRF protection enabled and provide the normal CSRF cookie or token. API routes retain their existing authentication and authorization requirements. See [middleware](../the-basics/middleware.md) and [validation](../the-basics/validation.md).
