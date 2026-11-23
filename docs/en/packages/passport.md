# Passport

## Installation {#installation}

For a default application, run:

```sh
./artisan install:api --passport --without-migration-prompt --no-interaction
./artisan passport:install --no-interaction
```

The API installer enables the bundled `passport` Cargo feature, preserves other default features, adds the default User's `HasApiTokens` implementation and the `api` Passport guard, and creates/registers `routes/api.rs`. It rebuilds the application, invokes `passport:install`, then rebuilds the published configuration and migrations. Cargo feature selection and compilation replace Composer installation; there is no `--composer` option. Set `APP_KEY` and the database connection before running migrations. The default user endpoint uses `auth:api` and respects `API_PREFIX`, including nested prefixes.

`passport:install` preserves an existing complete key pair, publishes configuration and five migrations, and registers the configuration loader. In a terminal it asks to migrate and create a personal client (both default yes). `--no-interaction` runs both steps; each successful rerun can create another personal client. `--without-migration-prompt` publishes only and reports migrations pending, so run the second command when ready. This explicit skip option is a native addition to the package installer. Without a terminal or `--no-interaction`, it reports pending work without prompting. Production migration confirmation still applies; use an explicit `artisan migrate --force` deployment step and create the personal client separately with `artisan passport:client --personal --no-interaction`.

Existing routes, configuration and migration files are preserved. API `--force` replaces only the API routes stub and keeps keys. Package `passport:install --force --length=2048` explicitly regenerates keys. An incomplete key pair fails with recovery instructions. Build, publication, configuration, migration and client creation failures return a nonzero status; fix the reported problem and retry. Custom model/guard/configuration source without recognized insertion points requires manual integration and is reported as incomplete. For an application with its own setup, the manual steps below remain available.


## Deploying Passport {#deploying-passport}

Generate the default 4096-bit RSA pair:

```sh
./artisan passport:keys
```

Keys are stored in the application's configured storage directory as `oauth-private.key` and `oauth-public.key`. On Unix their modes are 0600 and 0660; Windows uses its filesystem access controls. Existing keys cause exit code 1. `--force` replaces them, and `--length=2048` selects a different size. Restart services after replacing keys. Concurrent non-force generators produce at most one matching pair.

```sh
./artisan passport:keys --force --length=2048
./artisan vendor:publish --tag=passport-migrations
cargo build
./artisan migrate
```

All five migrations, including `oauth_device_codes`, are published with migration timestamps. Rebuild before migrating because Rust discovers migration source at compile time. The provider does not automatically register or execute this package schema. Repeated publication preserves existing files unless `--force` is specified. Use the migration commands documented under [Migrations](../database/migrations.md) to inspect, roll back or rebuild the schema.

## Configuration {#configuration}

The package supplies `guard = "web"`, an empty middleware list, and nullable key and connection settings. Application configuration overrides package defaults. `PASSPORT_PRIVATE_KEY` and `PASSPORT_PUBLIC_KEY` accept inline RSA PEM, literal `\n` line breaks, absolute local paths or `file://` paths (including `file://localhost/` on Unix). Relative paths are rejected, following the fixed PHP local-file wrapper behavior; they are not silently anchored to the application root. With no key configured, the storage files are loaded lazily. This supports RSA PKCS#1 / PKCS#8 private keys and RSA / SubjectPublicKeyInfo public keys; it does not claim the full generic EC-key API of League's CryptKey. No raw key contents are included in debug output or loader errors. Online RS256 uses AWS-LC with 2048–8192-bit RSA keys.

Publish the configuration when you need compiled application overrides:

```sh
./artisan vendor:publish --tag=passport-config
```

Add `mod passport;` to `config/mod.rs`, and `("passport".to_owned(), passport::load(env)?),` to its configuration entries, then rebuild. This is the Rust equivalent of loading the published PHP configuration file. The published loader and package defaults share the same implementation. `config:cache` retains the resulting values; clear the cache before loading changed environment values.

`PASSPORT_CONNECTION` chooses a named connection for all five migration definitions; null uses the application's default connection. Their ledger stays on the connection selected by the migration command, following the framework. The migrations use the owning application's configuration explicitly, so concurrent applications cannot exchange connection names.

Resolve `Passport` during provider boot to choose a custom key directory before the key services are first resolved:

```rust
use openrust::foundation::Application;
use openrust_passport::Passport;

pub async fn configure_keys(app: &Application) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let passport = app.make::<Passport>().await?;
    passport.load_keys_from(passport.key_path("keys"));
    Ok(())
}
```

The directory must exist and be writable for generation. This uses an application-owned `Passport` instance instead of PHP's process-global mutable settings. `key_path` and `load_keys_from` retain their names. Keys load independently and lazily; configuration alone does not assert that the public and private keys form a pair. On Unix, file modes outside 0400, 0440, 0600, 0640 and 0660 fail validation. An application can explicitly disable that check with `passport.validate_key_permissions.store(false, std::sync::atomic::Ordering::Relaxed)`; other applications keep their own setting. Inline keys and Windows skip this Unix mode check.

The default and standard-size key generation uses the existing native AWS-LC implementation. Other supported lengths use a separate generator. RustCrypto is used for generation and PEM parsing, not online RSA decryption or signing. No PHP or OpenSSL command-line installation is required.

## Managing clients {#managing-clients}

The client repository supports lookup, creation, update, secret rotation, owner filtering and revocation using real Eloquent models. `passport:client` creates the six supported client grant types. Authorization-code and device endpoints and resource token authentication are available.

Resolve `ClientRepository` inside the request or Artisan lifecycle. The container gives each lifecycle a new repository. Repeated `find` calls with the same argument share the actual model, including cached misses; database errors propagate and can be retried. `find_active` also checks the current model's `revoked` attribute. Do not retain this scoped service in an application singleton. For an explicitly managed operation, `Passport::client_repository()` creates a separately owned cache.

The following helper is called inside that lifecycle after publishing and running the client migration:

```rust
use openrust::foundation::Application;
use openrust_passport::{ClientOwner, ClientRecord, ClientRepository};

pub async fn create_client(
    app: &Application,
    owner: Option<&ClientOwner>,
) -> Result<ClientRecord, Box<dyn std::error::Error + Send + Sync>> {
    let clients = app.make::<ClientRepository>().await?;
    Ok(clients
        .create_authorization_code_grant_client(
            "My application",
            &["https://example.com/callback".to_owned()],
            true,
            owner,
            false,
        )
        .await?)
}
```

The arguments retain order. Rust makes default arguments explicit: `true` creates a confidential client, `owner` is optional, and the final `false` disables the additional device grant. A `ClientOwner` is made with `ClientOwner::for_user(&user)` from a compiled `Model + Authenticatable`; it uses the model's primary key and morph class, rather than its possibly different authentication identifier. Public clients pass `false` for confidentiality.

`create_personal_access_grant_client`, `create_password_grant_client`, `create_client_credentials_grant_client`, `create_implicit_grant_client` and `create_device_authorization_grant_client` select their corresponding grants. Creating a row does not enable the optional password or implicit protocols; device routes follow the application device switch. `update` changes the name and redirects; `regenerate_secret` creates a new secret. Both save through Eloquent and return its boolean result. `find_for_user` and `for_user` exclude revoked clients; the latter sorts by name. `personal_access_client(provider)` selects the latest matching active personal client, including a null provider only when it matches `auth.guards.api.provider`, and returns an error when none exists.

Confidential secrets use the application's existing asynchronous Eloquent hash service. The creating or rotating `Client` temporarily retains its setter input in `plain_secret`; fresh database reads do not. Default JSON output hides `secret` and does not append the plaintext. Copy a newly generated plaintext only when delivering it to its owner, and never log it. An `Arc<tokio::sync::Mutex<M>>` returned by `record.model::<M>()` refers to the actual selected model; `same_instance` checks shared object identity, not whether two model rows have the same key.

Use `client.append(&["plain_secret"])` to explicitly append the current instance’s temporary plaintext. A freshly hydrated model appends null. Appended values still obey `make_hidden` and `set_visible`.

`ClientModel::has_scope` and `ClientRecord::has_scope` allow all scopes when the raw `scopes` attribute is absent or null. Otherwise, they check the configured array using PHP non-strict comparison. Parent-scope inheritance is disabled by default. Calling `passport.with_inherited_scopes.store(true, std::sync::atomic::Ordering::Relaxed)` on the current application’s instance lets `orders` grant descendants such as `orders:read`. Here, `*` is an ordinary scope name. The switch belongs to each application instance. Call these methods within that application’s request or Artisan lifecycle.

Call `passport.use_client_model::<CustomClient>()` during application setup before resolving repositories. The compiled type implements `ClientModel` and Eloquent `Model`; its unique-ID validation, global scopes, events and casts remain in effect. Malformed UUID/ULID keys return no result before connection resolution. Repository queries and unbound `Client::query()`/`save()` use an explicit model connection first, then `passport.connection`, then the database default. Created and loaded models retain their actual connection, so later configuration changes do not redirect their saves. `Client::query_on(&connection)` selects a connection explicitly. Transaction writes check the effective model connection and reject a transaction from another connection. Rust exposes `resolve_connection_name()` with an optionally owned name for application configuration and `get_connection()` for the actual connection. Secret assignment uses PHP scalar string coercion. Failed hashing preserves the stored hash and retains the current setter input in `plain_secret`. Mass assignment follows Rust’s existing attribute key order and preserves earlier successful assignments if a later setter fails.

## Client factories

`Client::factory()?` constructs `database::factories::ClientFactory<Client>`. Use `as_public`, `as_password_client`, `as_personal_access_token_client`, `as_implicit_client`, `as_client_credentials`, or `as_device_code_client` to apply the corresponding grant state. Later states override earlier attributes. The default is a confidential authorization code client with a random 40-character secret and one generated redirect URI. Owners are null and revoked is false.

Call `make().await?` for an unsaved model or `create().await?` to persist it. Assignment uses the model’s asynchronous mutators and casts: the new Client retains `plain_secret`, while a database reload contains only its hash. `state` adds application attributes; `into_factory()` exposes the existing Eloquent builder for counts, relationships and callbacks.

Rust selects a factory model at compile time: use `ClientFactory::<CustomClient>::new()?` for a custom `ClientModel`. This does not dynamically read `use_client_model`. Construction resolves and captures the model’s effective connection, including `passport.connection`, and returns configuration errors. Construct a fresh factory after changing that configuration. Authorization code, refresh and device protocols are available.

## Client commands and revocation

After running the migrations, create clients with `./artisan passport:client`. Options `--personal`, `--password`, `--client`, `--implicit` and `--device` select the grant; without these flags the command creates an authorization code client. If several flags are present, that order determines precedence. `--name`, `--provider` and `--redirect_uri` supply prompt values; redirects are comma separated. `--public` omits the secret for password, device and authorization code clients. Implicit clients are always public; personal and client credentials clients are confidential.

With `--no-interaction`, the name defaults to `app.name`, the callback to the application URL plus `/auth/callback`, and the additional device flow to false. Interactive authorization code creation asks about device flow when `device_code_grant_enabled` is true (the default). The command prints the client ID and, for confidential clients, the temporary secret and a reminder to retain it. Personal client creation prints only its success message.

`clients.delete(&client).await?` loads the client's access tokens in batches of 1000 and revokes each refresh token before its access token, then revokes the client. These are Eloquent saves with casts and events. Errors propagate and earlier saves remain; a save cancelled by an event returns false and does not stop the chain, matching Passport. Authorization codes are unchanged. `TokenModel::revoke` and `RefreshTokenModel::revoke` also expose individual revocation.

## Models and relationships

`Token`, `RefreshToken` and `AuthCode` use string keys without auto increment, boolean `revoked` and datetime `expires_at` casts, and the Passport connection fallback. Only `Token` uses timestamps; its scopes are an array. `Token::can` rejects empty scopes, recognizes the PHP loose `*` wildcard and uses the application inheritance setting; `cant` negates it.

`ClientModel` exposes `tokens`, `auth_codes`, `owner` and the deprecated `user::<U>()`; `Token` exposes `client`, `refresh_token` and the deprecated `user::<U>("authentication_key")`; `RefreshToken` exposes `access_token`, and `AuthCode` exposes `client`. The token user key must match the compiled provider model's `Authenticatable` identifier; the client user relation uses its primary key. Register polymorphic owners through the existing `owner().target::<U>(alias, primary_key)` builder.

Rust binds relation result types at compile time. Default helpers return the default concrete models; use `tokens_for::<T>`, `client_for::<C>`, `refresh_token_for::<R>` or `access_token_for::<T>` for custom models. `passport.use_token_model::<T>()` and `use_refresh_token_model::<R>()` select the actual models used by repository revocation, including their connections and events. These shared registrations also apply to already resolved repositories. The caller chooses the compiled user provider model instead of PHP's runtime class string.

`client_uuids` defaults to true. Set `passport.client_uuids.store(false, std::sync::atomic::Ordering::Relaxed)` before constructing clients when using an existing numeric auto increment client table. Each instance retains the strategy selected at construction. This does not change an existing schema; the published client migration creates UUID keys.

## Scope JSON

`Scope::to_json()` retains the `id`, `description` field order and PHP’s default Unicode and slash escaping. Use `to_json_with_options(ScopeJsonOptions)` for `JSON_HEX_TAG`, `JSON_HEX_AMP`, `JSON_HEX_APOS`, `JSON_HEX_QUOT`, `JSON_NUMERIC_CHECK`, `JSON_UNESCAPED_SLASHES`, `JSON_PRETTY_PRINT`, `JSON_UNESCAPED_UNICODE`, `JSON_PRESERVE_ZERO_FRACTION` and `JSON_UNESCAPED_LINE_TERMINATORS`. Combine constants with `|`; `from_bits` accepts PHP option bits. Flags for invalid UTF-8 have no effect on Rust’s valid UTF-8 strings.

```rust
use openrust_passport::{Scope, ScopeJsonOptions};
let scope = Scope::new("orders/read", "Read orders");
let json = scope.to_json_with_options(
    ScopeJsonOptions::JSON_UNESCAPED_UNICODE | ScopeJsonOptions::JSON_UNESCAPED_SLASHES,
)?;
```

## Authorization code grant {#authorization-code-grant}

The provider registers GET, POST and DELETE `/oauth/authorize` with the `web` middleware and session locking, plus throttled POST `/oauth/token`. Set `passport.path` to change the `oauth` prefix and `passport.middleware` to add middleware to all these routes. Call `passport.ignore_routes()` before the package boots to register your own routes. Browser authentication uses `passport.guard` (default `web`); configure the application's normal login route and SessionGuard user provider. Publish and run the migrations, set `APP_KEY`, and generate the RSA keys first.

During application setup, resolve `Passport` and call `passport.tokens_can(BTreeMap::from([("read".to_owned(), "Read data".to_owned())]))`, `passport.set_default_scope(vec!["read".to_owned()])` if needed, and `passport.authorization_view(consent)`. These settings belong to the application. The callback below returns consent data for an application UI; render the client, scopes and approve/deny controls in that UI:

```rust
use openrust::http::{HttpError, HttpFuture, Request, Response, StatusCode};
use openrust::contracts::session::Session as _;
use openrust_passport::http::AuthorizationViewParameters;

pub fn consent(request: &mut Request, params: AuthorizationViewParameters)
    -> HttpFuture<'_, Result<Response, HttpError>>
{
    Box::pin(async move {
        Response::json(StatusCode::OK, serde_json::json!({
            "auth_token": params.auth_token,
            "csrf": request.session()?.token(),
            "user": params.user.get_auth_identifier(),
            "scopes": params.scopes,
        }))
    })
}
```

Submit `auth_token` and the session CSRF `_token` to POST `/oauth/authorize` to approve, or DELETE to deny. The browser must retain its session cookie. The server consumes the saved request and nonce once and checks the current authenticated subject; submitted client, redirect and scope fields cannot replace them. Approval rechecks the current client and registered callback. Denial redirects to the validated callback with `error=access_denied` and the original `state`.

Clients must generate a fresh unpredictable `state` and check it on the callback before accepting a code or error. Confidential clients authenticate the token exchange with HTTP Basic credentials or body `client_id` and `client_secret`. Body credentials take precedence. Parameters come from the request body, not the query. Successful responses contain `token_type`, `expires_in`, `access_token` and, for clients allowing refresh, `refresh_token`, with `Cache-Control: no-store` and `Pragma: no-cache`. Access tokens use RS256. Authorization codes expire after ten minutes and are consumed atomically with token persistence; competing exchanges have one winner. Access and refresh lifetimes default independently to one calendar year and can be configured as described below.

`prompt=none` redirects with `login_required` or `consent_required` when interaction is needed. Existing unrevoked, unexpired grants can skip consent for already granted scopes. `prompt=consent` requests consent again; `prompt=login` requests fresh session authentication. Invalid clients and unregistered redirects fail locally without an untrusted Location header. OAuth errors retain their protocol fields and status codes.

## Authorization code grant with PKCE {#code-grant-pkce}

Create a public client with `passport:client --public`. It must send a challenge; confidential clients can also use PKCE. Generate a fresh 43–128 character verifier from letters, digits and `-._~`, then compute its SHA-256 and base64url encoding without padding. Use `S256`:

```text
GET /oauth/authorize?response_type=code&client_id=CLIENT_ID&redirect_uri=https%3A%2F%2Fclient.test%2Fcb&scope=read&state=RANDOM_STATE&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256
```

After approval, exchange the returned code with the original verifier and exactly the same supplied redirect URI:

```text
POST /oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code&client_id=CLIENT_ID&redirect_uri=https%3A%2F%2Fclient.test%2Fcb&code=AUTHORIZATION_CODE&code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk
```

These are fixed example values; generate new values for each real authorization. Missing or incorrect verifiers, altered callbacks, expired codes and replayed codes fail without issuing tokens. The authorization step permits different ports for registered HTTP loopback callbacks; the token exchange binds the exact redirect string originally supplied.

Rust stores a validated native request in Session and invokes a compiled view callback instead of PHP object serialization. Codes and refresh tokens use the existing application's authenticated encryption plus application context; their internal envelope is not PHP ciphertext compatible. Client/code locks and a single database transaction keep code consumption, access-token and refresh-token saves atomic. Custom token models participate through Eloquent casts, events and their actual connection; save cancellation or database failure rolls back issuance.

## Device authorization grant {#device-authorization-grant}

Create a device client with `./artisan passport:client --device --public`. Confidential device clients omit `--public`. Device authorization is enabled by default. To disable its routes, set `passport.device_code_grant_enabled.store(false, std::sync::atomic::Ordering::Relaxed)` during provider boot before Passport assembles routes. Changing the setting after application boot does not remove registered routes.

POST `/oauth/device/code` with `client_id` and optional space-separated `scope`. The response contains `device_code`, `user_code`, `verification_uri`, `verification_uri_complete`, `expires_in` (600 seconds) and `interval` (5 seconds). This creation step identifies the client without validating a secret. Keep `device_code` on the device; show the user the verification URI and user code, or open the complete URI on a browser. The response uses `no-store` and `no-cache`.

Configure both `passport.device_user_code_view(code_view)` and `passport.device_authorization_view(consent)` during application setup. `code_view` has the signature `fn(&mut Request) -> HttpFuture<'_, Result<Response, HttpError>>`; the consent callback uses the same `AuthorizationViewParameters` signature as `authorization_view` above. The application renders its own HTML, including escaped client and scope values. GET `/oauth/device` renders the code entry view; its GET form sends `user_code` back to that URL. Hyphens are removed from entered codes; case is preserved. `verification_uri_complete` skips code entry and redirects to GET `/oauth/device/authorize`, which requires the configured session user and renders consent.

The consent form sends the saved `auth_token` and session CSRF `_token` to POST `/oauth/device/authorize` to approve, or DELETE to deny. The browser keeps its session cookie. The server binds the saved request to that authenticated user, consumes its nonce once, and prevents competing browsers from changing an already decided device code. It redirects to `/oauth/device` with session `status` equal to `authorization-approved` or `authorization-denied`. Invalid codes return there with old `user_code` input and validation errors. Missing view callbacks fail explicitly.

The device polls POST `/oauth/token` with `grant_type=urn:ietf:params:oauth:grant-type:device_code`, `device_code` and `client_id`. Confidential clients also authenticate with their secret, using Basic or body credentials. Wait at least the advertised interval: HTTP 400 `authorization_pending` means consent is still pending; HTTP 400 `slow_down` requires slowing polling. Both persist the latest poll time. Denial returns HTTP 401 `access_denied`; expiry returns HTTP 400 `expired_token`. A successful exchange atomically persists access/refresh tokens and revokes the device code; replay fails. Access-token scope and lifetime handling uses the ordinary resource guard and refresh flow. `passport.path` applies to all device routes. `DeviceCode` is a real Eloquent model with scope, boolean and datetime casts and the Passport connection fallback.

## Refreshing tokens {#refreshing-tokens}

POST `/oauth/token` with `grant_type=refresh_token`, `refresh_token` and the client's credentials. Public clients send `client_id`; confidential clients also authenticate with their secret. Omit `scope` to keep the original scopes, or request a subset. An empty parameter uses the original scopes under the normal OAuth parameter rules. Additional scopes are rejected.

A successful refresh revokes the old access and refresh tokens and returns a new pair. Rotation and persistence share a database transaction: concurrent requests have one winner, and a failed or cancelled save preserves the old pair. Refresh tokens remain usable after their access token expires or is individually revoked. Revoking a refresh token prevents redemption without revoking its access token.

Access and refresh lifetimes default independently to one calendar year. Configure them on the application's `Passport` instance before issuing tokens. `None` reads the current lifetime; `Some` updates it. `GrantLifetime::Interval(DateInterval)` supports signed calendar components through the framework Date implementation, preserving the application's timezone, daylight saving transitions and PHP month overflow.

```rust
use openrust_passport::GrantLifetime;
passport.tokens_expire_in(Some(GrantLifetime::Seconds(3600)));
passport.refresh_tokens_expire_in(Some(GrantLifetime::CalendarYears(1)));
let current = passport.tokens_expire_in(None);
```

## Revoking tokens {#revoking-tokens}

Use `TokenModel::revoke` or `RefreshTokenModel::revoke` for an Eloquent instance. For the authenticated request, clone `TokenGuard::token(request, "api")` and call `token.revoke().await?`. This updates the selected access-token store. Repository client revocation still follows the refresh → access → client lifecycle described above.

## Purging tokens {#purging-tokens}

Run `artisan passport:purge`. It removes revoked items and items expired for more than 168 hours. `--revoked` or `--expired` selects one criterion; both flags combine them with OR. `--hours=24` changes the expired-item retention period. Access-token rows with an unrevoked refresh token whose expiry is null or in the future are retained. Auth codes and refresh tokens are also purged; device codes are included when the device grant is enabled. Custom token and refresh model tables, connections and query scopes participate.

## Protecting routes {#protecting-routes}

Set the guard in `config/auth.rs` to `{"driver":"passport","provider":"users"}` and apply `auth:api` to protected routes. The provider must resolve the application's compiled user model. RS256 signature, optional JWT time claims and the selected access-token revocation store are checked on every request. Inactive clients, provider mismatch and missing users are unauthenticated. Provider lookup exceptions are treated as unauthenticated; failures of the authoritative token store fail the request.

The real user remains available through `Auth::user(request).await?`. Rust attaches the lightweight access token to the request under its guard name instead of replacing the compiled user object. Read it with `openrust_passport::guards::TokenGuard::token(request, "api")`; `can`, `cant` and `transient` expose scope checks. `ResourceServer::validate_authenticated_request` also inserts typed `AccessToken` request attributes. Guard resolution is cached within each request and isolated by guard name and application. Cookie authentication and scope middleware are described below.

## Client credentials grant {#client-credentials-grant}

Create a confidential machine client with `artisan passport:client --client`. Send `grant_type=client_credentials`, `client_id`, `client_secret` and optional space-delimited `scope` to `POST /oauth/token`. HTTP Basic credentials are also accepted. The response has `access_token`, `token_type` and `expires_in`, with `Cache-Control: no-store`; it has no refresh token. Public clients are rejected. The JWT subject is the client ID and the database user ID is null.

Protect machine routes with `EnsureClientIsResourceOwner::shared(["read"])`. This verifies the token and required scopes without requiring a user provider. User guards still require a real user. `passport.client_credentials_tokens_expire_in(Some(GrantLifetime::Seconds(300)))` sets the machine lifetime; until configured it follows `tokens_expire_in`.

## Personal access tokens {#personal-access-tokens}

Create a personal client with `artisan passport:client --personal --provider=users`. Within the application context, call `PersonalAccessTokenFactory::make(user_id, "Automation", &["read".into()], "users").await?`. The dedicated personal grant is unavailable at the public token endpoint. Client selection follows the provider; a client with no provider is a fallback only for the configured API provider.

A compiled user model may implement `HasApiTokens`, explicitly returning its configured provider from `get_provider_name`, then call `user.create_token("Automation", &["read".into()]).await?`. Rust requires this declaration instead of inferring a PHP model class name. Tokens preserve their user, name and scopes and have no refresh token. `personal_access_tokens_expire_in` defaults to one calendar year independently of other grants.

`PersonalAccessTokenResult` exposes `access_token_id`, `access_token`, `token_type` and `expires_in` through serialization, `to_array` and `to_json`. `get_token().await?` loads and caches the actual default `Token` as `Option<PersonalToken<Token>>`; this is an `Arc<tokio::sync::Mutex<Token>>`, so repeated access shares the same model. For a compiled custom token model use `get_token_for::<CustomToken>()`. Lock the model to revoke it with the `TokenModel` trait. Debug output redacts the bearer token.

## Token scopes {#token-scopes}

Register scope descriptions with `tokens_can(BTreeMap::from([("read".into(), "Read data".into())]))`. `set_default_scope(vec!["read".into()])` sets defaults for requests that omit `scope`. Explicit unknown scopes are rejected; client scope restrictions are applied before signing. Machine tokens may retain `*`. `default_scopes(None)` reads defaults and `default_scopes(Some(scopes))` updates them. The protocol normalizes the resulting space-delimited string.

`scope_ids`, `has_scope`, `valid_scopes` and `scopes_for` query the configured scopes. `valid_scopes` filters and deduplicates in input order; `scopes_for` retains duplicates and filters unknown IDs. `has_scope("*")` is true, while `valid_scopes` includes `*` only if explicitly configured.

```rust
use openrust_passport::http::middleware::{CheckToken, CheckTokenForAnyScope};
route.middleware(CheckToken::shared(["read", "write"]));
route.middleware(CheckTokenForAnyScope::shared(["read", "write"]));
```

These compiled middleware values replace PHP middleware strings. `CheckToken` requires all scopes; `CheckTokenForAnyScope` requires at least one. An empty all list passes and an empty any list fails. Missing authentication returns 401 and insufficient scope returns 403, `Invalid scope(s) provided.`. `exceptions::MissingScopeException` exposes the native error type. `TokenGuard::current_token(request)` reads the selected guard's token. Transient first-party tokens authorize all scopes.

## SPA authentication {#spa-authentication}

Add `CreateFreshApiToken::using(Some("web"))` as shared middleware inside the web stack, after Session and cookie middleware. Authenticated GET responses receive the encrypted HttpOnly `openrust_token` cookie, unless the response already sets it. The signed internal JWT carries the actual user identifier, CSRF token and expiry. Its lifetime and cookie attributes follow Session configuration. `passport.cookie(Some("custom_name"))` changes its name.

Use a Passport API guard for API resources. The API stack normally excludes the web `EncryptCookies` middleware: Passport decrypts its own cookie. If an application already decrypts the cookie before the guard, explicitly call `without_cookie_encryption`. `with_cookie_encryption`, `with_cookie_serialization`, `without_cookie_serialization`, `encrypt_tokens_using` and `token_encryption_key` configure the matching producer/consumer format and key. Settings belong to the application instance; applications using the same key and format may read each other's cookies.

Send the Session CSRF value in `X-CSRF-TOKEN`, or the encrypted XSRF cookie in `X-XSRF-TOKEN`, even for cookie-authenticated API GET requests. Invalid signatures, expiry, CSRF or missing users are unauthenticated. An explicit Bearer header takes precedence and cannot fall back to the cookie. `ignore_csrf_token(true)` is an explicit bypass and defaults to false.

`POST /oauth/token/refresh` uses the web Session and CSRF middleware and returns 200 `Refreshed.` with a fresh cookie. Cookie authentication creates a request-local transient token without a database access-token row. Install `CreateFreshApiToken` on the web logout route too: it observes the authenticated-to-guest transition and expires the API cookie. This native adaptation replaces PHP's global-request logout listener; routes without this middleware do not automatically clear that cookie.

## Events {#events}

Listen to the typed `events::AccessTokenCreated`, `RefreshTokenCreated` and `AccessTokenRevoked` through the framework dispatcher. Payloads contain token/client/user identifiers, never the bearer or client secret. Access-token creation is dispatched after saving the access model, followed by refresh-token creation after its save. A synchronous listener failure rolls back issuance; `Delivery::AfterCommit` observes committed records. Refresh rotation emits revocation in its transaction. Direct access-token `revoke` updates storage without emitting that rotation event.

## Testing {#testing}

Within the application context, call `passport.acting_as(&mut request, Box::new(user), &["read".into()], "api", None).await?` for a user or `passport.acting_as_client(&mut request, &client, &["read".into()], "api").await?` for a machine. These helpers select the guard and attach a scope-aware token to that explicit request. They create no signed token or database record and affect no subsequent request. `acting_as_client` requires a request without a cached user; use a fresh request when switching from user authentication. Real protocol tests must still obtain and submit signed tokens.

## Password grant {#password-grant}

Password and Implicit grants are disabled by default. Creating their client records does not enable the protocols. For an integration that explicitly requires Password, call `passport.enable_password_grant()` on the application instance before resolving its authorization server. `password_grant_enabled` remains application-local; setting it to false rejects subsequent Password requests. Authorization code with PKCE remains the normal browser flow.

Create a client with `artisan passport:client --password --provider=users` (`--public` creates a public client). Send `grant_type=password`, `client_id`, `username`, `password` and optional `scope` to `POST /oauth/token`; confidential clients also provide `client_secret` or HTTP Basic credentials. Success returns a signed access token and, when the client supports it, a refresh token. Ordinary access/refresh lifetimes apply. Incorrect user credentials return 400 `invalid_grant`, `The user credentials were incorrect.`; missing fields, invalid clients and unknown scopes retain OAuth error responses.

`bridge::UserRepository::get_user_entity_by_user_credentials` selects the client's provider, falling back to `auth.guards.api.provider` for a null/empty provider. The default compiled Eloquent provider queries `email` and verifies the real password hash. A custom framework `UserProvider` supplies the native equivalents of model `findForPassport` and `validateForPassportPasswordGrant` through `retrieve_by_credentials` and `validate_credentials`. This path neither starts a Session nor rehashes passwords.

For a combined model-level `findAndValidateForPassport` hook, register `passport.find_and_validate_for_passport_using("users", resolver)`. The native `bridge::PasswordGrantResolver` is a compiled function taking borrowed username, password and `ClientRecord`, returning `AuthFuture<AuthResult<Option<AuthIdentifier>>>`. It takes precedence for that provider; it must perform the application's actual lookup and validation. This explicit registration replaces PHP runtime method discovery. Exceptions fail with a redacted 500 response; they do not become invalid passwords.

## Implicit grant {#implicit-grant}

Enable only an explicitly required integration with `passport.enable_implicit_grant()`, then create its public client with `artisan passport:client --implicit --redirect_uri=https://client.example/callback`. Send `response_type=token`, `client_id`, optional `redirect_uri`, `scope`, `state` and `prompt` to `GET /oauth/authorize`. The existing web Session, CSRF, authorization view, user binding and single-use consent nonce apply. This optional legacy flow is not a replacement for the PKCE example above.

Approval returns a 302 redirect to the validated callback with `access_token`, `token_type`, `expires_in` and supplied `state` in its fragment. No auth code or refresh token is created. Unknown-scope, login-required, consent-required and denial redirects also use the fragment; invalid clients or untrusted callbacks fail locally. Existing callback query parameters are preserved. Scope finalization removes `*` for Implicit and device authorization; Password, PAT and client credentials retain it. Client scope restrictions still apply.

`implicit_grant_enabled` is application-local. Setting it false also rejects completion of a pending Implicit consent request. The native implementation rechecks the active client and registered redirect before committing a token, and its transaction rolls back on model/listener failure. A consent POST cannot change the stored client, user, scopes or response type. `grant_type=implicit` is never accepted by the token endpoint.

## Overriding default models {#overriding-default-models}

Configure the application’s `Passport` instance before serving requests with `use_client_model::<C>()`, `use_token_model::<T>()`, `use_refresh_token_model::<R>()`, `use_auth_code_model::<A>()` and `use_device_code_model::<D>()`. The corresponding `client_model`, `token_model`, `refresh_token_model`, `auth_code_model` and `device_code_model` getters return the selected compiled type name. Registrations belong to each application. These methods select real Eloquent models for protocol persistence, resource validation, revocation and `passport:purge`; they do not copy records into the default tables.

Client, access and refresh models implement `ClientModel`, `TokenModel` and `RefreshTokenModel` respectively in addition to `Model`. Auth code and device models implement `Model`. Retain the base model’s casts, primary-key behavior, guarded/hidden attributes and timestamps in your metadata, and supply your own schema. For example, a custom client still hides `secret`; registering a model does not repair its serialization policy. Model observers and save cancellation remain effective, including rollback during authorization and token consumption. Missing tables and storage failures are errors, and can be retried after correcting the setup.

Use typed relationships for custom targets: `client.auth_codes_for::<A>()`, `client.tokens_for::<T>()`, `code.client_for::<C>()`, `token.client_for::<C>()`, `token.refresh_token_for::<R>()` and `refresh.access_token_for::<T>()`. The default relationship helpers keep their concrete default return types. PAT results use `get_token_for::<T>()`; factories select a type with `ClientFactory::<C>::new()`.

An explicit model connection takes precedence over `passport.connection`, then the protocol’s parent connection. OAuth writes in one issuance/consumption transaction must share that actual connection; crossing connection identities returns an error and rolls back, even when aliases point to the same database. Use `PASSPORT_CONNECTION` consistently for the OAuth tables. A user provider may independently query a different user store. Register compiled user models through `AuthManager::register_model` or a custom `UserProvider`, configure the guard’s provider, and preserve the distinction between a model primary key and its authentication identifier. Password provider hooks are described above.

## Overriding routes {#overriding-routes}

Set `passport.path` for a nested OAuth prefix and `passport.middleware` for common middleware. To replace package routes, call `passport.ignore_routes()` in application setup before routes load, then register the public controllers yourself. For example, a token-only integration can register:

```rust
use openrust::{routing::{RouteGroup, RoutingError}, support::facades::Route};

pub fn oauth_routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().prefix("integrations"), |router| {
        router.post("/token",
            openrust_passport::http::controllers::AccessTokenController::issue_token)?
            .name("passport.token")
            .without_middleware("web")
            .middleware_name("throttle");
        Ok(())
    })
}
```

This stateless token route accepts independent HTTP clients and returns the ordinary OAuth response and cache headers. Browser consent and device approval routes retain web Session, CSRF, authentication and blocking middleware, along with the package’s named routes. Device grant availability also requires the named `passport.device` route. Ignoring built-in routes does not implicitly install those browser routes or change the resource guard’s provider.

## Using Passport with Sanctum {#sanctum-coexistence}

Enable both `passport` and `sanctum` default Cargo features. Running the two API installers without `--force` preserves existing API routes; add the second guard’s resource routes explicitly and run its published migration. Configure a Passport guard such as `api` with its user provider and use `auth:api`; Sanctum routes use `auth:sanctum`. The default user may implement both packages’ `HasApiTokens`, using fully qualified calls to distinguish the methods:

```rust
let oauth = openrust_passport::HasApiTokens::create_token(&user, "OAuth", &[]).await?;
let personal = openrust::sanctum::HasApiTokens::create_token(
    &user, &sanctum, &database, "Sanctum", &["read"], None,
).await?;
```

Register the user for Sanctum with `AuthManager::register_sanctum_model` as well as the normal user provider. OAuth JWTs and Sanctum personal Bearer tokens use separate stores and guards, and are not interchangeable. Revoking one credential leaves the other package’s credential valid. Configure browser Session authentication separately. The request-local `acting_as` and `acting_as_client` helpers in the Testing section remain explicit substitutes within the selected request/guard; they do not install clients or create durable database tokens.
