# Session

## Configuration

Use `SESSION_DRIVER=database` to persist sessions in the configured database and `sessions` table. Apply the application migration before serving session-backed routes. The `array` driver keeps data in the current application process. Set `SESSION_ENCRYPT=true` to encrypt the stored payload.

The default `web` group starts the session before the handler when a session driver is configured. For custom groups, place `EncryptCookies` before `StartSession`.

## Interacting with the Session

A route stack with `EncryptCookies` followed by `StartSession` exposes the loaded store through `request.session()` and `request.session_mut()`. The middleware saves the store and attaches the session cookie after the route returns. Import the `Session` trait to use the store methods.

```rust
use openrust::{
    contracts::session::Session,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

fn remember(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let session = request.session_mut()?;
        session.put("profile.name", "Ada".into())?;
        session.flash("message", "Saved".into())?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

## Flash Data

`flash` makes a value available immediately and on the next request. Use `keep` for selected keys or `reflash` for all flash data to retain it for another request.

## Regenerating the Session ID

`regenerate(true)` removes the previous ID from storage, preserves attributes and rotates the CSRF token. Use `invalidate()` to clear attributes and rotate the ID.

## Session Blocking

By default, requests using the same session can run concurrently. Call `block(lock_seconds, wait_seconds)` when registering a session route to serialize requests that use the same session ID.

```rust
use openrust::support::facades::Route;

Route::getWith("/profile", remember, |route| {
    route.block(10, 10);
    Ok(())
})?;
```

The first argument is the lock lifetime and the second is the maximum time to wait for a held lock. The middleware acquires the lock before reading the session and releases it after saving it. A lock timeout goes through the exception handler and returns HTTP 500 by default.

Set `SESSION_BLOCK=true` to apply blocking to every route that starts a session. Configure the defaults with `SESSION_BLOCK_LOCK_SECONDS` and `SESSION_BLOCK_WAIT_SECONDS` (both 10 seconds). A route's explicit settings override the defaults. `withoutBlocking()` clears the route-specific settings. When global blocking is enabled, the route still uses the global settings.

Set `SESSION_BLOCK_STORE` to a cache store that supports atomic locks, such as `redis`. The default cache store is used when this option is unset. Use a shared store across application processes. The `array` cache only coordinates requests within one process.
