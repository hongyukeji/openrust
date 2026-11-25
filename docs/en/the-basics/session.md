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

`only` selects top-level keys. `except` removes keys or dotted paths from a copy. `exists` and `missing` distinguish an explicitly null value from an absent key, while `has` and `has_any` require a non-null value. Use `remember` with a fallible closure to compute and store a value only when the key is absent or null.

Use `push` to append to an array or associative array. `increment` and `decrement` take an explicit integer amount and return the new JSON value. An invalid input returns an error and preserves the original entry.

Use `put_many` to set multiple keys, including dotted paths. `replace` updates the supplied keys and keeps the other attributes. Use `has_old_input(Some(key))` to check for a non-null flashed field, or `has_old_input(None)` to check whether the old input bag has any entries.

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

Route::get_with("/profile", remember, |route| {
    route.block(10, 10);
    Ok(())
})?;
```

The first argument is the lock lifetime and the second is the maximum time to wait for a held lock. The middleware acquires the lock before reading the session and releases it after saving it. A lock timeout goes through the exception handler and returns HTTP 500 by default.

Set `SESSION_BLOCK=true` to apply blocking to every route that starts a session. Configure the defaults with `SESSION_BLOCK_LOCK_SECONDS` and `SESSION_BLOCK_WAIT_SECONDS` (both 10 seconds). A route's explicit settings override the defaults. `without_blocking()` clears the route-specific settings. When global blocking is enabled, the route still uses the global settings.

Set `SESSION_BLOCK_STORE` to a cache store that supports atomic locks, such as `redis`. The default cache store is used when this option is unset. Use a shared store across application processes. The `array` cache only coordinates requests within one process.

## Adding Custom Session Drivers

Implement `SessionHandler` to read, write, destroy and collect session records. The handler stores bytes supplied by the framework. Register a factory with `Session::extend` in a service provider's `boot` method. It receives the current container and request, and creates a handler for that request.

```rust
use std::time::Duration;
use openrust::{
    foundation::Application,
    http::HttpFuture,
    session::ArraySessionHandler,
    support::{ProviderResult, ServiceProvider, facades::Session},
};

struct AppSessionProvider;

impl ServiceProvider for AppSessionProvider {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            let storage = ArraySessionHandler::new(Duration::from_secs(120 * 60));
            Session::extend("app_memory", move |_, _| Ok(storage.clone()))?;
            Ok(())
        })
    }
}
```

Register this provider with the application and set `SESSION_DRIVER=app_memory`. The example keeps records in the application process. A shared backend can persist records across processes.

## Default cookie name

The default is `Str::slug(APP_NAME) + "-session"`: `My App` becomes `my-app-session`, while `ExampleStore` becomes `examplestore-session`. Upgrading changes the old default cookie name and makes existing browser sessions unavailable. Set `SESSION_COOKIE` explicitly to retain the old name. Cache and Redis prefixes use the same slug; set `CACHE_PREFIX` and `REDIS_PREFIX` explicitly to retain existing namespaces.


## Inspecting session configuration

`SessionManager::should_block()` reads `session.block`; `block_driver()` reads `session.block_store`. `default_route_block_lock_seconds()` and `default_route_block_wait_seconds()` return the configured lock and wait durations, each defaulting to 10 seconds. `get_session_config()` returns the configuration snapshot used when assembling this manager, including custom keys. Changes to the configuration repository after assembly do not update this snapshot.

## Session facade {#session-facade}

Import `support::facades::Session` to access the session already loaded for the current HTTP request. Reads, writes, flash data, old input, CSRF tokens, and ID rotation use the same Store as `StartSession`; middleware still persists it at the end of the request. Calls without an active request or configured session return an error instead of creating an empty session.

```rust
use openrust::{
    http::HttpError,
    support::facades::{Route, Session},
};

Route::get("/visits", || async {
    let visits = Session::increment("visits", 1)?;
    Session::flash("message", serde_json::json!("Saved"))?;
    Ok::<_, HttpError>(visits)
})?;
```

`get` returns `Option<Value>`. `get_or` applies its default only to a missing key; an explicit `null` remains `null`. `all`, `only`, and `except` return owned copies rather than references escaping the request borrow. `regenerate`, `migrate`, `invalidate`, `start`, and `save` are asynchronous. Handlers borrowing `&mut Request` should use that instance or lend it safely through `request.with_container(async { ... }).await` before calling the facade.

## Read-only database session metadata {#database-session-metadata}

For authorized account-management code, `SessionManager::database_handler()` creates a fresh native database handler using the configured connection, table and lifetime. It returns an error for other session drivers or a custom replacement registered under `database`.

`DatabaseSessionHandler::sessions_for(user_id)` reads unexpired candidates through the `user_id` index and returns their ID, IP address, User-Agent and Unix-second activity time, newest first. Treat them as candidates: a shared `user_id` does not establish the authenticated Guard or user identity.

Load each candidate with `SessionManager::read_database_session(&id).await?`. This uses native decryption and Store decoding without saving, refreshing activity, collecting garbage or replacing the current request's session. Invalid IDs are rejected; missing, expired or undecodable JSON sessions have no authenticated login attributes. Check `SessionGuard::session_belongs_to_user(config, guard, user, &attributes)?` before selecting a candidate. It requires the matching Guard login identity and current credential fingerprint; missing, forged or stale fingerprints fail.

These lower-level APIs return sensitive internal IDs and attributes to trusted server code. A public device response needs its own authorization and opaque identifiers; never serialize a complete session or expose its raw ID, CSRF token or login fingerprint. Device revocation and account-management routes are separate lifecycle operations.

## Additional session identity policies {#session-identity-policies}

Register an application-owned asynchronous policy through `Auth::add_session_validator("device-policy", callback)?`. The callback receives the actual Guard name, resolved user and read-only native Store, and returns `AuthFuture<AuthResult<bool>>`. The normal password/credential checks still run. `false` rejects authentication with 401 and clears the resolved device state; an error propagates without a permissive fallback. The callback also runs for remember recovery before the session ID is regenerated, so it can inspect the original session.

Names make registration idempotent: replacing a name retains its registration position, different names run in order and stop on rejection or error. `Auth::remove_session_validator("device-policy")?` returns whether a policy was removed. Registration, replacement and removal affect only that application's AuthManager. Callbacks are cloned out of the registry before awaiting; a request retains its normal cached identity after successful resolution. This is an authentication boundary for new resolutions, not retroactive cancellation of executing requests.

The policy is applied to session Guards created by AuthManager. A low-level `SessionGuard::new` constructed directly has no application registry. No configured policy preserves native behavior. These hooks provide the connection point for durable device rules; applications must still implement authorized persistent state and transaction rules for revocation.
