# Authentication

## Guards and User Providers

The `auth.guards` configuration selects a guard and its provider. The `session` guard uses the current web request’s session. Attach the `auth` middleware alias to protected routes and `guest` to routes for guests. Read the resolved identity with `Auth::guard(request, None)?.user().await?`. Each request has its own resolved identities and selected guard.

## Manually Authenticating Users

Build `Credentials` from application-selected lookup columns and a password. `attempt` verifies credentials, rehashes the password when configured, and rotates the session ID and CSRF token on success. Use `attempt_throttled` in a login handler and supply a key derived from the normalized account identifier and a trusted client address. The limiter atomically charges attempts, returns 429 with `Retry-After` when exhausted, and clears the counter after verification.

```rust
use openrust::{
    auth::{AuthResult, Credentials},
    http::Request,
    support::facades::Auth,
};

async fn sign_in(
    request: &mut Request,
    credentials: &Credentials,
    throttle_key: &str,
    remember: bool,
) -> AuthResult<bool> {
    Auth::session_guard(request, None)?
        .attempt_throttled(credentials, remember, throttle_key, 5, 60)
        .await
}
```

## Remembering Users and Logging Out

Pass `true` for remember to issue an encrypted, HTTP-only cookie. The guard can restore a session using the persisted remember token and a fingerprint of the current password hash. `logout` removes the current guard and rotates the token. Use `logout_and_invalidate` when the application also needs to discard the entire session and rotate its CSRF token.

## HTTP Basic Authentication {#http-basic-authentication}

Attach `auth.basic` to a web route to authenticate with a session guard. The default lookup field is `email`. Use `auth.basic:web,username` to select a guard and a different field. Invalid or missing credentials produce HTTP 401 with `WWW-Authenticate: Basic`. An existing valid session also satisfies the middleware.

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register_basic_routes() -> Result<(), RoutingError> {
    Route::get_with("/profile", || view("profile"), |route| {
        route.middleware_name("auth.basic");
        Ok(())
    })?;
    Route::get_with("/profile/by-name", || view("profile"), |route| {
        route.middleware_name("auth.basic:web,username");
        Ok(())
    })
}
```

Call `Auth::once_basic(request, "email", &[]).await?` in an API handler for request-only authentication. Use HTTPS when sending Basic credentials.

## Authentication events {#authentication-events}

The application dispatcher emits `Attempting` before a credential attempt and `Validated` after the provider accepts the credentials. Persistent login emits `Login` then `Authenticated`. Resolving a session emits `Authenticated`. Restoring a remembered session emits `Login` with `remember = true`. A failed attempt emits `Failed`, which can include the user found by the provider. `Logout` and `CurrentDeviceLogout` identify how authentication ended.

Synchronous listeners for `Attempting` and `Failed` can access the borrowed credentials through `credentials(context)`. Keep passwords out of logs. A `Validated` listener can borrow the typed user through `user::<T>(context)`.

A throttled login emits `Lockout` with its guard and retry delay. A synchronous listener can borrow the request metadata through `request(context)`.

```rust
use std::sync::Arc;

use openrust::{
    auth::events::{Lockout, Login},
    contracts::events::{Dispatcher as _, Event},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
};

fn record_auth_event<'a>(
    event: &'a dyn Event,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        if let Some(login) = event.downcast_ref::<Login>() {
            eprintln!("login guard={} remember={}", login.guard, login.remember);
        } else if let Some(lockout) = event.downcast_ref::<Lockout>() {
            eprintln!("lockout guard={} retry_after={}", lockout.guard, lockout.retry_after);
        }
        Ok(None)
    })
}

fn register_auth_events(dispatcher: &Dispatcher) -> EventResult<()> {
    dispatcher.listen("Login", Arc::new(record_auth_event), Delivery::Sync)?;
    dispatcher.listen("Lockout", Arc::new(record_auth_event), Delivery::Sync)
}
```

## Invalidating sessions on other devices {#invalidating-sessions-on-other-devices}

Add `auth.session` after `auth` on routes that should enforce session invalidation. To sign out other devices, validate the request in an authenticated Web handler and call `Auth::logout_other_devices(request, password).await?`. The guard verifies the current password, updates its stored hash, refreshes the current remember cookie if needed, and emits `OtherDeviceLogout`. Other sessions carrying the previous fingerprint are rejected when they next visit a protected route.

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register_account_routes() -> Result<(), RoutingError> {
    Route::get_with("/account", || view("account"), |route| {
        route.middleware_name("auth").middleware_name("auth.session");
        Ok(())
    })
}
```

## Password confirmation {#password-confirmation}

Protect sensitive Web routes with `password.confirm` after `auth`. The middleware uses `auth.password_timeout` in seconds (10,800 by default). Use `password.confirm:password.confirm,300` to override the confirmation route and timeout. When confirmation is required, browsers are redirected to the named route and the intended URL is saved in the session. JSON requests receive HTTP 423.

Make the confirmation form a CSRF-protected POST. `Auth::confirm_password` verifies the current account through its user provider and records the successful confirmation. A custom confirmation handler can verify the password itself and call `request.session_mut()?.password_confirmed()?`.

```rust
use openrust::{
    auth::Auth,
    http::{HttpError, HttpFuture, Request, Response, StatusCode, abort},
    routing::{RouteGroup, RoutingError},
    support::facades::{Redirect, Route},
    view::view,
};

fn confirm(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let password = request.input(Some("password")).await?;
        let password = password.as_ref().and_then(serde_json::Value::as_str)
            .ok_or(HttpError::InvalidRequest)?;
        if !Auth::confirm_password(request, password).await? {
            return Err(abort(StatusCode::UNPROCESSABLE_ENTITY, "Invalid password.", Default::default()));
        }
        Ok(Redirect::intended(request, "/settings", Default::default())?.into())
    })
}

fn register_password_routes() -> Result<(), RoutingError> {
    Route::get_with("/confirm-password", || view("auth.confirm-password"), |route| {
        route.name("password.confirm").middleware_name("auth");
        Ok(())
    })?;
    Route::group(
        RouteGroup::new().middleware_name("auth").middleware_name("throttle:6,1"),
        |router| {
            router.post("/confirm-password", confirm)?;
            Ok(())
        },
    )?;
    Route::get_with("/settings", || view("settings"), |route| {
        route.middleware_name("auth").middleware_name("password.confirm");
        Ok(())
    })
}
```

## Custom Guards and Providers

Authentication models are selected in `config/auth.rs`, following `auth.providers` configuration. `openrust::auth::model!` is the Rust counterpart of a PHP model class reference: it returns the class name and retains its compiled factory. Neither `AppServiceProvider` nor `bootstrap/app.rs` needs model registration:

```rust
let user = openrust::auth::model!(crate::app::models::User);
let admin = openrust::auth::model!(crate::app::models::Admin);

let providers = serde_json::json!({
    "users": {"driver": "eloquent", "model": user},
    "admins": {"driver": "eloquent", "model": admin},
});
```

Declare `Admin` in your application's models module before using it. Adding a model class reference requires rebuilding. `AUTH_MODEL` can select any referenced, compiled model; an unknown class fails when its Eloquent provider is resolved. The linker retains factories even when configuration caching skips the loader. Factories are resolved lazily without querying user tables, and mutable authentication state stays within its application. An explicit second argument, such as `model!(Admin, "app::models::Admin")`, sets a stable class alias. `AuthManager::register_model` remains available for explicit runtime extensions. Alternatively, configure a `database` provider with a table and column names. Register a provider implementation with `provider` and a stateless request resolver with `via_request`. The built-in `token` guard reads the Bearer header and supports hashing tokens before database lookup.

For an identity registered with `AuthManager::register_model`, `manager.user_morph_class(user)?` returns its actual Eloquent morph discriminator, including the model’s connection and custom `get_morph_class` behavior. This Rust adapter preserves morph-map errors and returns `None` for unregistered identities. Configure non-model causers explicitly instead of guessing a table or type name.

Synchronous callbacks can use `manager.resolved_user(None)` or `Auth::resolved_user(None)?` to read an identity already resolved in the current HTTP request, without enabling log listeners. `None` follows `should_use` and the default guard; `Some("web")` selects that guard explicitly. These Rust adapters do not run asynchronous authentication or query a user provider. An unresolved guard, guest, or call outside the owning request scope returns `None`; callbacks needing lazy authentication must resolve the guard first in asynchronous middleware.

## Request authentication caches {#request-auth-cache}

After committing an identity change, reset resolved guards before resolving the user again:

```rust
use openrust::support::facades::Auth;

Auth::forget_guards(request)?;
let current = Auth::user(request).await?;
```

This follows the pinned `AuthManager::forgetGuards` contract. Rust keeps these caches on the supplied request, preserving the selected `should_use` guard, application default and in-progress resolver protection. Cached users and cached guests are resolved again on demand; other requests and applications are unaffected. `AuthManager::forget_guards` is also available directly. The operation itself does not log out a session, rotate CSRF/Cookies or revoke tokens. Account actions perform those changes explicitly, then reset the cache.
