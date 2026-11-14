# Sanctum

## Installing API routes {#installation}

Run `./artisan install:api` to create `routes/api.rs` and register it in the application. Build with `cargo build --features sanctum` to enable the generated `GET /api/user` endpoint and the compiled migration. In a Sanctum-enabled binary, the interactive installer offers to run pending migrations. For CI, use `--without-migration-prompt` and run `artisan migrate` separately. Use `--force` to replace an existing API routes file with the generated template.

## Token table migration {#token-table-migration}

Build the application with the `sanctum` Cargo feature to register the compiled `personal_access_tokens` migration. Use the resulting binary for migration and rollback.

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

The migration creates a polymorphic owner, a unique 64-character token hash column, nullable abilities and timestamps, and an expiration index. The feature also registers the Sanctum guard and CSRF cookie route.

## Issuing personal access tokens {#issuing-api-tokens}

Create a token with the token store resolved by `Sanctum::personalAccessTokens`. Supply the owner type, owner ID, name, abilities and optional expiration. Give the client the returned `plain_text_token` once. The database stores its SHA-256 digest. `findToken` resolves credentials, and `revokeToken` deletes a token scoped to its owner. `can` accepts an exact ability or the wildcard `*`. The HTTP guard checks expiration before authenticating the request.

```rust
use openrust::{auth::AuthResult, database::DatabaseManager, sanctum::{NewAccessToken, Sanctum}};

async fn issue(sanctum: &Sanctum, database: &DatabaseManager, user_id: u64) -> AuthResult<NewAccessToken> {
    sanctum.personalAccessTokens(database, "")?
        .createToken("app::models::User", user_id, "browser", &["reports:read"], None)
        .await
}
```

In a service provider, call `usePersonalAccessTokenModel::<M>()` on the application’s `Sanctum` service to select the Eloquent model’s table and connection. Create the custom table in an application migration using the standard Sanctum columns and integer `id`. Issuance applies model mutators and casts before saving and fires the Eloquent creation events. Bearer authentication reads through the model’s accessors and casts and saves the last-used timestamp through the same model instance. `revokeToken` deletes the owner’s model instance and honors its deletion events. Artisan pruning applies the model’s global scopes and deletes in bulk without firing per-token events.

## Protecting routes and checking abilities {#protecting-routes}

Attach `auth:sanctum` to protected routes. Send the credential in `Authorization: Bearer <token>`. Invalid, expired and revoked credentials receive HTTP 401. Use `abilities:reports:read,reports:write` to require both abilities or `ability:reports:read,reports:write` to require either. An authenticated token that lacks a required ability receives HTTP 403.

```rust
use openrust::{http::ApiResponse, routing::RoutingError, support::facades::Route};

fn register() -> Result<(), RoutingError> {
    Route::getWith("/reports", || ApiResponse::success(["quarterly"]), |route| {
        route.middleware_name("auth:sanctum").middleware_name("abilities:reports:read");
        Ok(())
    })
}
```

Use the registered provider model name as the token owner type. The default application uses `app::models::User`. A custom Sanctum guard can set its `provider` in `auth.guards`; when omitted, the guard resolves the owner through the registered Eloquent models and `MorphMap`. Use `AccessToken::currentAccessToken(request)` to inspect the request's credential. Set `sanctum.expiration` in minutes to cap token age independently of per-token expiry. Successful Bearer authentication dispatches `TokenAuthenticated` and updates `last_used_at`. Set `sanctum.last_used_at` to false to disable the timestamp update.

Register `getAccessTokenFromRequestUsing` to replace Bearer extraction. Register `authenticateAccessTokensUsing` to decide whether a found token is valid. The callback receives the token and the built-in expiration and provider decision. The owner must still resolve.

## SPA authentication {#spa-authentication}

In the existing `withMiddleware` closure in `bootstrap/app.rs`, call `middleware.statefulApi();`. Set `SANCTUM_STATEFUL_DOMAINS` to the comma-separated first-party frontend hosts, including any non-default port. With the default route configuration, request `GET /sanctum/csrf-cookie`, then sign in through the application's web session login. Send cookies on subsequent API requests and pass the URL-decoded `XSRF-TOKEN` cookie value in the `X-XSRF-TOKEN` header for requests that require CSRF protection.

For a cross-origin first-party frontend, enable credentialed CORS for its exact origin. A matching Referer or Origin activates the session, cookie and CSRF middleware. Sanctum checks the authenticated session before the Bearer token. First-party sessions pass token ability checks. Apply application authorization policies to control what the user can do.

After a successful first-party request records the password fingerprint, a password change invalidates that session on its next request. To replace a frontend middleware, register it with the application's middleware `alias` method and use the alias as the value of `sanctum.middleware.encrypt_cookies`, `sanctum.middleware.validate_csrf_token`, or `sanctum.middleware.authenticate_session`.

Add `__SANCTUM_CURRENT_REQUEST_HOST__` to `SANCTUM_STATEFUL_DOMAINS` or append `Sanctum::currentRequestHost()` to a comma-separated domain string to match the current request host and port.

## Pruning expired tokens {#pruning-expired-tokens}

Run the command with the Sanctum feature enabled. The default retention is 24 hours. It prunes by explicit expiry and, when `sanctum.expiration` is set, by creation time plus that lifetime. The application accepts `SANCTUM_EXPIRATION` in minutes.

```sh
./artisan sanctum:prune-expired --hours=24
```
