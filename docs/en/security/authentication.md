# Authentication

## Guards and User Providers

The `auth.guards` configuration selects a guard and its provider. The `session` guard uses the current web request’s session. Attach the `auth` middleware alias to protected routes and `guest` to routes for guests. Read the resolved identity with `Auth::guard(request, None)?.user().await?`. Each request has its own resolved identities and selected guard.

## Manually Authenticating Users

Build `Credentials` from application-selected lookup columns and a password. `attempt` verifies credentials, rehashes the password when configured, and rotates the session ID and CSRF token on success. Use `attemptThrottled` in a login handler and supply a key derived from the normalized account identifier and a trusted client address. The limiter atomically charges attempts, returns 429 with `Retry-After` when exhausted, and clears the counter after verification.

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
    Auth::sessionGuard(request, None)?
        .attemptThrottled(credentials, remember, throttle_key, 5, 60)
        .await
}
```

## Remembering Users and Logging Out

Pass `true` for remember to issue an encrypted, HTTP-only cookie. The guard can restore a session using the persisted remember token and a fingerprint of the current password hash. `logout` removes the current guard and rotates the token. Use `logoutAndInvalidate` when the application also needs to discard the entire session and rotate its CSRF token.

## HTTP Basic Authentication {#http-basic-authentication}

Attach `auth.basic` to a web route to authenticate with a session guard. The default lookup field is `email`. Use `auth.basic:web,username` to select a guard and a different field. Invalid or missing credentials produce HTTP 401 with `WWW-Authenticate: Basic`. An existing valid session also satisfies the middleware.

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register_basic_routes() -> Result<(), RoutingError> {
    Route::getWith("/profile", || view("profile"), |route| {
        route.middleware_name("auth.basic");
        Ok(())
    })?;
    Route::getWith("/profile/by-name", || view("profile"), |route| {
        route.middleware_name("auth.basic:web,username");
        Ok(())
    })
}
```

Call `Auth::onceBasic(request, "email", &[]).await?` in an API handler for request-only authentication. Use HTTPS when sending Basic credentials.

## Authentication events {#authentication-events}

The application dispatcher emits `Attempting` before a credential attempt and `Validated` after the provider accepts the credentials. Persistent login emits `Login` then `Authenticated`. Resolving a session emits `Authenticated`. Restoring a remembered session emits `Login` with `remember = true`. A failed attempt emits `Failed`, which can include the user found by the provider. `Logout` and `CurrentDeviceLogout` identify how authentication ended.

Synchronous listeners for `Attempting` and `Failed` can access the borrowed credentials through `credentials(context)`. Keep passwords out of logs. A `Validated` listener can borrow the typed user through `user::<T>(context)`.

```rust
use std::sync::Arc;

use openrust::{
    auth::events::Login,
    contracts::events::{Dispatcher as _, Event},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
};

fn record_login<'a>(
    event: &'a dyn Event,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        if let Some(login) = event.downcast_ref::<Login>() {
            eprintln!("login guard={} remember={}", login.guard, login.remember);
        }
        Ok(None)
    })
}

fn register_auth_events(dispatcher: &Dispatcher) -> EventResult<()> {
    dispatcher.listen("Login", Arc::new(record_login), Delivery::Sync)
}
```

## Custom Guards and Providers

The default application registers its compiled Eloquent `User`. Register other models with `AuthManager::registerModel`, or configure a `database` provider with a table and column names. Register a provider implementation with `provider` and a stateless request resolver with `viaRequest`. The built-in `token` guard reads the Bearer header and supports hashing tokens before database lookup.
