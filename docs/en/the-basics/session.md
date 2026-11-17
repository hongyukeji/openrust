# Session

## Configuration

Use `SESSION_DRIVER=database` to persist sessions in the configured database and `sessions` table. Apply the application migration before serving session-backed routes. The `array` driver keeps data in the current application process. Set `SESSION_ENCRYPT=true` to encrypt the stored payload.

When an authentication guard is configured, the database driver writes the selected guard's user ID to `user_id` when the request ends. Guest requests write `NULL`. A custom session table must use a `user_id` column type that fits the application's user identifiers.

Use `SESSION_DRIVER=file` to keep sessions in `storage/framework/sessions`. Set `session.files` in application configuration to use another directory. The `native` driver is an alias for `file`. Use `SESSION_DRIVER=null` to expose session state for the current request and discard it when the request ends.

Use `SESSION_DRIVER=redis`, `memcached`, or `dynamodb` to store sessions in a cache backend. `SESSION_STORE` selects a named cache store and defaults to the driver name. For Redis, `SESSION_CONNECTION` selects a named Redis connection and defaults to `default`. The `session.prefix` configuration can override the Redis cache prefix for sessions. Session settings are applied to a dedicated Redis repository, so the ordinary cache keeps its own connection and prefix. The backend expires payloads after `SESSION_LIFETIME` minutes.

Use `SESSION_DRIVER=cookie` to store the session in two encrypted cookies. The configured session cookie carries the ID. The cookie named by that ID carries the payload and its expiry. `SESSION_EXPIRE_ON_CLOSE` sets browser-session cookies while preserving the payload expiry. The encrypted payload cookie has a 4096-byte header limit. Rotation with destruction expires the old payload cookie in the current browser. A copied cookie pair can be replayed until its expiry. Session blocking requires a server-side driver.

The default `web` group starts the session before the handler when a session driver is configured. For custom groups, place `EncryptCookies` before `StartSession`. The Cookie driver also needs `AddQueuedCookiesToResponse` between them.

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

## Session Cache {#session-cache}

Use `request.session()?.cache()?` on a route that starts a session. It resolves the same repository as `Cache::store(Some("session"))`. Entries follow the active session and support the usual cache operations and TTLs. `flush()` clears that cache namespace while preserving ordinary session attributes and other cache namespaces. Regenerating the ID preserves cached values, and invalidating the session clears them.

```rust
use openrust::{
    cache::Ttl,
    contracts::cache::Repository as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

fn cache_banner(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let cache = request.session()?.cache()?;
        cache.put("dashboard.banner", &"ready".into(), Some(Ttl::Seconds(300))).await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

`SESSION_CACHE_KEY` selects the namespace and defaults to `_cache`. The session driver persists the entries along with session data. Cache operations require an active HTTP session, so `./artisan cache:clear session` cannot clear these entries. For requests that must serialize writes to the same session, use session blocking with a lock-capable store.

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
