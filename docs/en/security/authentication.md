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

## Custom Guards and Providers

The default application registers its compiled Eloquent `User`. Register other models with `AuthManager::registerModel`, or configure a `database` provider with a table and column names. Register a provider implementation with `provider` and a stateless request resolver with `viaRequest`. The built-in `token` guard reads the Bearer header and supports hashing tokens before database lookup.
