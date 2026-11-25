# Sanctum

## Installing API routes {#installation}

The default API routes/configuration are available without Sanctum. Run `./artisan install:api` to enable Sanctum in the application’s default Cargo features and add its protected `/user` route to the unchanged empty skeleton. If the file already contains business routes, the installer preserves it and you must add the protected route manually. The installer offers to run pending migrations in an interactive terminal. If you accept, it rebuilds the application before migrating. For CI, use `--without-migration-prompt` and run `./artisan migrate` separately. Use `--force` to replace an existing API routes file. If the installer cannot update a custom bootstrap, it preserves the generated route and prints the manual registration instruction. The route returns the registered authenticated model’s visible attributes. If an optional migration fails, the installer displays the error and retains the API scaffolding. Run `./artisan migrate` separately when automation needs the migration exit code.

The default application skeleton ships without Sanctum-specific configuration or user model registration. After enabling the Cargo feature, the package supplies its default configuration. In a new application, `install:api` publishes the token migration and wires the published configuration and default user model. A custom model must implement `HasApiTokens` and be referenced with `openrust::auth::model!(YourUser, sanctum)` in `config/auth.rs`. Explicit runtime extensions may use `AuthManager::register_sanctum_model`. The installer updates the model capability and configuration class reference, preserving `AppServiceProvider`. When enabling the feature manually, publish migrations with `vendor:publish --tag=sanctum-migrations` and editable configuration with `vendor:publish --tag=sanctum-config`. After manual publication, declare the module in `config/mod.rs` and register its loader result under `sanctum`.

Run `vendor:publish` in a terminal to choose a provider or tag. Package providers register compiled files in `boot` with `publishes` or `publishes_migrations`. A new migration gets a fresh timestamp when `database.migrations.update_date_on_publish` is enabled. Disable it or call `VendorPublishCommand::dont_update_migration_dates` in a provider to preserve source dates. Republishing reuses the existing filename, so the migration ledger stays stable. Rebuild the application after publishing a migration to `database/migrations/` to include it in the compiled list.

## Token table migration {#token-table-migration}

Once published, the migration is part of the application. The next build includes it in the migration registry even when the `sanctum` Cargo feature is disabled.

```sh
./artisan install:api --without-migration-prompt
./artisan migrate
./artisan migrate:status
```

The migration creates a polymorphic owner, a unique 64-character token hash column, nullable abilities and timestamps, and an expiration index. The feature also registers the Sanctum guard and CSRF cookie route.

The published migration uses `table.morphs("tokenable")`. It creates an integer `tokenable_id` by default. For UUID or ULID owner models, call `Schema::morph_using_uuids()?` or `Schema::morph_using_ulids()?` in your application service provider's `boot` before the first migration. Import `Schema` from `openrust::database::schema`. This default applies to subsequent `morphs` calls in the application. The token table's own primary key remains an integer.

## Issuing personal access tokens {#issuing-api-tokens}

Create a token with the token store resolved by `Sanctum::personal_access_tokens`. Supply the owner type, owner ID, name, abilities and optional expiration. Give the client the returned `plain_text_token` once. The database stores its SHA-256 digest. `find_token` resolves credentials, and `revoke_token` deletes a token scoped to its owner. `can` accepts an exact ability or the wildcard `*`. The HTTP guard checks expiration before authenticating the request. Token issuance, authentication updates and individual revocation use the Eloquent lifecycle. The default token response includes `updated_at`.

```rust
use openrust::{auth::AuthResult, database::DatabaseManager, sanctum::{NewAccessToken, Sanctum}};

async fn issue(sanctum: &Sanctum, database: &DatabaseManager, user_id: u64) -> AuthResult<NewAccessToken> {
    sanctum.personal_access_tokens(database, "")?
        .create_token("app::models::User", user_id, "browser", &["reports:read"], None)
        .await
}
```

In a service provider, call `use_personal_access_token_model::<M>()` on the application’s `Sanctum` service to select the Eloquent model’s table and connection. Create the custom table in an application migration using the standard Sanctum attributes and the primary key declared by the model. Issuance applies model mutators and casts before saving and fires the Eloquent creation events. Bearer authentication reads through the model’s accessors and casts and saves the last-used timestamp through the same model instance. `revoke_token` deletes the owner’s model instance and honors its deletion events. Artisan pruning applies the model’s global scopes and deletes in bulk without firing per-token events.

For UUID or ULID keys, declare the corresponding column in the migration and use `ModelMetadata::unique_id(UniqueId::Uuid)` or `UniqueId::Ulid` on the model. Use `primary_key` for a key column other than `id`. The credential stays in `<key>|<secret>` format. Token and owner identifiers use `AuthIdentifier` to retain their numeric or text representation. For a string owner key, use a compatible `tokenable_id` column and pass the owner’s identifier to issuance and revocation.

SQLite, MySQL and PostgreSQL support integer, UUID and ULID token keys with matching migrations and model metadata. Model casts, owner-scoped queries, revocation and pruning use the same interfaces. PostgreSQL `timestamp` columns accept database datetime text and ISO 8601 text; a column without a time zone keeps the local date/time and does not retain the offset. Pass datetime text rather than an untyped Unix integer to such a column.

Set `SANCTUM_TOKEN_PREFIX` to prepend an application-specific prefix to newly issued secrets.

Implement `PersonalAccessTokenModel` on the model. Its default `find_token` verifies the SHA-256 digest. An override receives the presented credential and the configured Eloquent query, and can call `find_token_by_digest` after adding constraints or decoding the credential. The override is responsible for validating the credential it accepts.

The application service exposes its registered model type through `personal_access_token_model()`. Use `current_application_url_with_port()` to obtain the configured application host and explicit port, prefixed with a comma for use in stateful domain strings.

## Model token relationship {#model-token-relationship}

Implement `HasApiTokens` on the Eloquent owner. Use `user.tokens(&sanctum, &database)?` to list or revoke its tokens and `user.create_token(&sanctum, &database, name, abilities, expires_at)` to issue one. The relationship uses the configured token model and the owner’s morph alias and primary key. `delete_token` and `delete` use scoped bulk deletion. A persisted token can resolve its owner with `tokenable`. Read the request’s token and abilities through `current_access_token`, `token_can` and `token_cant` on the owner.

For a chainable Eloquent query, use `user.tokens_query::<DefaultPersonalAccessToken>(&sanctum, &database)?`. Use `user.tokens_relation::<DefaultPersonalAccessToken>(&sanctum)?` with Eloquent’s `with("tokens", relation)` to eager-load. Substitute the registered model type for a custom model. The nested token in the issuance response uses the model’s casts, accessors, appended fields, and visibility, and always omits the digest. Hide the `token` field in custom models when serializing them directly.

## Protecting routes and checking abilities {#protecting-routes}

Attach `auth:sanctum` to protected routes. Send the credential in `Authorization: Bearer <token>`. Invalid, expired and revoked credentials receive HTTP 401. Use `abilities:reports:read,reports:write` to require both abilities or `ability:reports:read,reports:write` to require either. An authenticated token that lacks a required ability receives HTTP 403.

```rust
use openrust::{http::ApiResponse, routing::RoutingError, support::facades::Route};

fn register() -> Result<(), RoutingError> {
    Route::get_with("/reports", || ApiResponse::success(["quarterly"]), |route| {
        route.middleware_name("auth:sanctum").middleware_name("abilities:reports:read");
        Ok(())
    })
}
```

Use the registered provider model name as the token owner type. The default application uses `app::models::User`. A custom Sanctum guard can set its `provider` in `auth.guards`; when omitted, the guard resolves the owner through the registered Eloquent models and `MorphMap`. Use `AccessToken::current_access_token(request)` to inspect the request's credential. Set `sanctum.expiration` in minutes to cap token age independently of per-token expiry. Successful Bearer authentication dispatches `TokenAuthenticated` and updates `last_used_at`. Set `sanctum.last_used_at` to false to disable the timestamp update.

Register `get_access_token_from_request_using` to replace Bearer extraction. Register `authenticate_access_tokens_using` to decide whether a found token is valid. The callback receives the token and the built-in expiration and provider decision. The owner must still resolve.

Register token-capable owners with `AuthManager::register_sanctum_model`. Use `NewAccessToken::to_json()` or `to_array()` explicitly to return the plaintext credential at issuance. The nested token omits its stored digest. The event payload is `TokenAuthenticated::token`.

## SPA authentication {#spa-authentication}

In the existing `with_middleware` closure in `bootstrap/app.rs`, call `middleware.stateful_api();`. Set `SANCTUM_STATEFUL_DOMAINS` to the comma-separated first-party frontend hosts, including any non-default port. With the default route configuration, request `GET /sanctum/csrf-cookie`, then sign in through the application's web session login. Send cookies on subsequent API requests and pass the URL-decoded `XSRF-TOKEN` cookie value in the `X-XSRF-TOKEN` header for requests that require CSRF protection.

For a cross-origin first-party frontend, enable credentialed CORS for its exact origin. A matching Referer or Origin activates the session, cookie and CSRF middleware. Sanctum checks the authenticated session before the Bearer token. First-party sessions pass token ability checks. Apply application authorization policies to control what the user can do.

After a successful first-party request records the password fingerprint, a password change invalidates that session on its next request. To replace a frontend middleware, register it with the application's middleware `alias` method and use the alias as the value of `sanctum.middleware.encrypt_cookies`, `sanctum.middleware.validate_csrf_token`, or `sanctum.middleware.authenticate_session`.

Add `__SANCTUM_CURRENT_REQUEST_HOST__` to `SANCTUM_STATEFUL_DOMAINS` or append `Sanctum::current_request_host()` to a comma-separated domain string to match the current request host and port.

## Pruning expired tokens {#pruning-expired-tokens}

Run the command with the Sanctum feature enabled. The default retention is 24 hours. It prunes by explicit expiry and, when `sanctum.expiration` is set, by creation time plus that lifetime. The application accepts `SANCTUM_EXPIRATION` in minutes.

```sh
./artisan sanctum:prune-expired --hours=24
```

## Testing {#testing}

Use `Sanctum::acting_as(&mut app, user, &["reports:read"], None)` with a `TestApp` and an owned Eloquent user. It returns a shared user whose `was_recently_created` flag has been cleared. It selects the Sanctum guard for subsequent requests from that fixture. Pass `Some("guard-name")` to select another guard and `["*"]` for all abilities. The fixture supplies the identity and abilities directly, without issuing a database token.

## Application-owned lifecycle policy {#credential-policy}

The optional [user starter](../getting-started/starter-kits.md#credential-lifecycle) adds durable revocation without changing standalone Sanctum defaults. `Sanctum::validate_token_owner_using` registers an application-scoped check after the standard token checks and owner resolution; its callback receives the token and the resolved user. `PersonalAccessToken::get_attribute_value` reads a custom model accessor/cast without serializing hidden attributes. Custom models with transient fields may override `PersonalAccessTokenModel::clone_token_model` to retain them.

For a custom workflow, `PersonalAccessTokens::create_token_in_transaction` accepts a transaction, owner type, owner identifier and `TokenOptions::new(name)`. The builder provides `abilities`, `expires_at`, `attributes` and `secret`. The transaction must belong to the same connection. Extra attributes cannot override credential fields, timestamps or the configured primary key, including JSON-path variants. This storage operation does not replace application authorization, recent authentication or the starter's user-row lock/version policy. Standard `create_token` remains sufficient for ordinary callers. Expiration and pruning use the application-owned clock, including `Date` test-clock overrides.

For account operations that share a transaction, `PersonalAccessTokens::find_owned_in_transaction` locks and reads only an owned token; `revoke_token_in_transaction` deletes through the configured model, preserving scopes, casts and model events. Both reject a different transaction connection. A cancelled deletion returns an error so the caller can roll back the complete operation. `Sanctum::validate_token_for_owner(token, owner, now)` reuses expiry and application callbacks when checking the freshly locked token; callers must supply the authenticated owner, verify exact ownership/provider and use their application clock. Normal applications should use the [user-kit endpoints](../getting-started/starter-kits.md#account-tokens).
