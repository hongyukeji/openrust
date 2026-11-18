# Sanctum

## Installing API routes {#installation}

Run `./artisan install:api` to create and register `routes/api.rs` and enable Sanctum in the application’s default Cargo features. The installer offers to run pending migrations in an interactive terminal. If you accept, it rebuilds the application before migrating. For CI, use `--without-migration-prompt` and run `./artisan migrate` separately. Use `--force` to replace an existing API routes file. If the installer cannot update a custom bootstrap, it preserves the generated route and prints the manual registration instruction. The route returns the registered authenticated model’s visible attributes. If an optional migration fails, the installer displays the error and retains the API scaffolding. Run `./artisan migrate` separately when automation needs the migration exit code.

The default application skeleton ships without Sanctum-specific configuration or user-model registration. The package supplies its defaults when the Cargo feature is enabled. On a fresh application, `install:api` publishes the token-table migration and connects the published configuration and the stock user model. If the user model or its provider is customized, implement `HasApiTokens` and register the token-capable model with `AuthManager::registerSanctumModel`. When enabling the feature manually, use `vendor:publish --tag=sanctum-migrations` for the migration and `vendor:publish --tag=sanctum-config` for an editable configuration. To use a manually published configuration, declare its module in `config/mod.rs` and include its loaded values under `sanctum`.

Run `vendor:publish` in a terminal to choose a provider or tag. Package providers register compiled files in `boot` with `publishes` or `publishesMigrations`. A new migration gets a fresh timestamp when `database.migrations.update_date_on_publish` is enabled. Disable it or call `VendorPublishCommand::dontUpdateMigrationDates` in a provider to preserve source dates. Republishing reuses the existing filename, so the migration ledger stays stable. Rebuild the application after publishing a migration to `database/migrations/` to include it in the compiled list.

## Token table migration {#token-table-migration}

Once published, the migration is part of the application. The next build includes it in the migration registry even when the `sanctum` Cargo feature is disabled.

```sh
./artisan install:api --without-migration-prompt
./artisan migrate
./artisan migrate:status
```

The migration creates a polymorphic owner, a unique 64-character token hash column, nullable abilities and timestamps, and an expiration index. The feature also registers the Sanctum guard and CSRF cookie route.

The published migration uses `table.morphs("tokenable")`. It creates an integer `tokenable_id` by default. For UUID or ULID owner models, call `Schema::morphUsingUuids()?` or `Schema::morphUsingUlids()?` in your application service provider's `boot` before the first migration. Import `Schema` from `openrust::database::schema`. This default applies to subsequent `morphs` calls in the application. The token table's own primary key remains an integer.

## Issuing personal access tokens {#issuing-api-tokens}

Create a token with the token store resolved by `Sanctum::personalAccessTokens`. Supply the owner type, owner ID, name, abilities and optional expiration. Give the client the returned `plain_text_token` once. The database stores its SHA-256 digest. `findToken` resolves credentials, and `revokeToken` deletes a token scoped to its owner. `can` accepts an exact ability or the wildcard `*`. The HTTP guard checks expiration before authenticating the request. Token issuance, authentication updates and individual revocation use the Eloquent lifecycle. The default token response includes `updated_at`.

```rust
use openrust::{auth::AuthResult, database::DatabaseManager, sanctum::{NewAccessToken, Sanctum}};

async fn issue(sanctum: &Sanctum, database: &DatabaseManager, user_id: u64) -> AuthResult<NewAccessToken> {
    sanctum.personalAccessTokens(database, "")?
        .createToken("app::models::User", user_id, "browser", &["reports:read"], None)
        .await
}
```

In a service provider, call `usePersonalAccessTokenModel::<M>()` on the application’s `Sanctum` service to select the Eloquent model’s table and connection. Create the custom table in an application migration using the standard Sanctum attributes and the primary key declared by the model. Issuance applies model mutators and casts before saving and fires the Eloquent creation events. Bearer authentication reads through the model’s accessors and casts and saves the last-used timestamp through the same model instance. `revokeToken` deletes the owner’s model instance and honors its deletion events. Artisan pruning applies the model’s global scopes and deletes in bulk without firing per-token events.

For UUID or ULID keys, declare the corresponding column in the migration and use `ModelMetadata::uniqueId(UniqueId::Uuid)` or `UniqueId::Ulid` on the model. Use `primaryKey` for a key column other than `id`. The credential stays in `<key>|<secret>` format. Token and owner identifiers use `AuthIdentifier` to retain their numeric or text representation. For a string owner key, use a compatible `tokenable_id` column and pass the owner’s identifier to issuance and revocation.

Set `SANCTUM_TOKEN_PREFIX` to prepend an application-specific prefix to newly issued secrets.

Implement `PersonalAccessTokenModel` on the model. Its default `findToken` verifies the SHA-256 digest. An override receives the presented credential and the configured Eloquent query, and can call `findTokenByDigest` after adding constraints or decoding the credential. The override is responsible for validating the credential it accepts.

The application service exposes its registered model type through `personalAccessTokenModel()`. Use `currentApplicationUrlWithPort()` to obtain the configured application host and explicit port, prefixed with a comma for use in stateful domain strings.

## Model token relationship {#model-token-relationship}

Implement `HasApiTokens` on the Eloquent owner. Use `user.tokens(&sanctum, &database)?` to list or revoke its tokens and `user.createToken(&sanctum, &database, name, abilities, expires_at)` to issue one. The relationship uses the configured token model and the owner’s morph alias and primary key. `deleteToken` and `delete` use scoped bulk deletion. A persisted token can resolve its owner with `tokenable`. Read the request’s token and abilities through `currentAccessToken`, `tokenCan` and `tokenCant` on the owner.

For a chainable Eloquent query, use `user.tokensQuery::<DefaultPersonalAccessToken>(&sanctum, &database)?`. Use `user.tokensRelation::<DefaultPersonalAccessToken>(&sanctum)?` with Eloquent’s `with("tokens", relation)` to eager-load. Substitute the registered model type for a custom model. The nested token in the issuance response uses the model’s casts, accessors, appended fields, and visibility, and always omits the digest. Hide the `token` field in custom models when serializing them directly.

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

Register token-capable owners with `AuthManager::registerSanctumModel`. Use `NewAccessToken::toJson()` or `toArray()` explicitly to return the plaintext credential at issuance. The nested token omits its stored digest. The event payload is `TokenAuthenticated::token`.

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

## Testing {#testing}

Use `Sanctum::actingAs(&mut app, user, &["reports:read"], None)` with a `TestApp` and an owned Eloquent user. It returns a shared user whose `wasRecentlyCreated` flag has been cleared. It selects the Sanctum guard for subsequent requests from that fixture. Pass `Some("guard-name")` to select another guard and `["*"]` for all abilities. The fixture supplies the identity and abilities directly, without issuing a database token.
