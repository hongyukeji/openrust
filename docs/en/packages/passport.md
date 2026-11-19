# Passport

## Installation {#installation}

Passport provides clients, browser authorization and consent, authorization codes with PKCE, and the authorization-code token endpoint. Resource token guards, refresh-token exchange and the remaining grants are under development; `install:api --passport` is not available yet. Existing Sanctum installation remains independent.

The current application has an optional `passport` Cargo feature. Add it to Cargo's default features to keep it enabled for `./artisan` and normal builds, preserving any existing defaults:

```toml
[features]
default = ["passport"]
```

Package discovery registers `openrust_passport::PassportServiceProvider`. Applications without discovery can register this provider with `withProviders`. Enabling the provider registers the implemented OAuth routes. Publish and migrate the schema, configure an application key and consent view, and generate keys before using them.

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

The package supplies `guard = "web"`, an empty middleware list, and nullable key and connection settings. Application configuration overrides package defaults. `PASSPORT_PRIVATE_KEY` and `PASSPORT_PUBLIC_KEY` accept inline RSA PEM, literal `\n` line breaks, absolute local paths or `file://` paths (including `file://localhost/` on Unix). Relative paths are rejected, following the fixed PHP local-file wrapper behavior; they are not silently anchored to the application root. With no key configured, the storage files are loaded lazily. This supports RSA PKCS#1 / PKCS#8 private keys and RSA / SubjectPublicKeyInfo public keys; it does not claim the full generic EC-key API of League's CryptKey. No raw key contents are included in debug output or loader errors.

Publish the configuration when you need compiled application overrides:

```sh
./artisan vendor:publish --tag=passport-config
```

Add `mod passport;` to `config/mod.rs`, and `("passport".to_owned(), passport::load(env)?),` to its configuration entries, then rebuild. This is the Rust equivalent of loading the published PHP configuration file. The published loader and package defaults share the same implementation. `config:cache` retains the resulting values; clear the cache before loading changed environment values.

`PASSPORT_CONNECTION` chooses a named connection for all five migration definitions; null uses the application's default connection. Their ledger stays on the connection selected by the migration command, following Laravel. The migrations use the owning application's configuration explicitly, so concurrent applications cannot exchange connection names.

Resolve `Passport` during provider boot to choose a custom key directory before the key services are first resolved:

```rust
use openrust::foundation::Application;
use openrust_passport::Passport;

pub async fn configure_keys(app: &Application) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let passport = app.make::<Passport>().await?;
    passport.loadKeysFrom(passport.keyPath("keys"));
    Ok(())
}
```

The directory must exist and be writable for generation. This uses an application-owned `Passport` instance instead of PHP's process-global mutable settings. `keyPath` and `loadKeysFrom` retain their names. Keys load independently and lazily; configuration alone does not assert that the public and private keys form a pair. On Unix, file modes outside 0400, 0440, 0600, 0640 and 0660 fail validation. An application can explicitly disable that check with `passport.validateKeyPermissions.store(false, std::sync::atomic::Ordering::Relaxed)`; other applications keep their own setting. Inline keys and Windows skip this Unix mode check.

The default and standard-size key generation uses the existing native AWS-LC implementation. Other supported lengths use a separate generator. RustCrypto is used for generation and PEM parsing, not online RSA decryption or signing. No PHP or OpenSSL command-line installation is required.

## Managing clients {#managing-clients}

The client repository supports lookup, creation, update, secret rotation, owner filtering and revocation using real Eloquent models. `passport:client` creates the six supported client grant types. Authorization-code endpoints are available; resource token authentication remains pending.

Resolve `ClientRepository` inside the request or Artisan lifecycle. The container gives each lifecycle a new repository. Repeated `find` calls with the same argument share the actual model, including cached misses; database errors propagate and can be retried. `findActive` also checks the current model's `revoked` attribute. Do not retain this scoped service in an application singleton. For an explicitly managed operation, `Passport::clientRepository()` creates a separately owned cache.

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
        .createAuthorizationCodeGrantClient(
            "My application",
            &["https://example.com/callback".to_owned()],
            true,
            owner,
            false,
        )
        .await?)
}
```

The arguments retain Laravel's order. Rust makes default arguments explicit: `true` creates a confidential client, `owner` is optional, and the final `false` disables the additional device grant. A `ClientOwner` is made with `ClientOwner::forUser(&user)` from a compiled `Model + Authenticatable`; it uses the model's primary key and morph class, rather than its possibly different authentication identifier. Public clients pass `false` for confidentiality.

`createPersonalAccessGrantClient`, `createPasswordGrantClient`, `createClientCredentialsGrantClient`, `createImplicitGrantClient` and `createDeviceAuthorizationGrantClient` select their corresponding grants. Password, implicit and device protocol endpoints are not enabled by creating a row. `update` changes the name and redirects; `regenerateSecret` creates a new secret. Both save through Eloquent and return its boolean result. `findForUser` and `forUser` exclude revoked clients; the latter sorts by name. `personalAccessClient(provider)` selects the latest matching active personal client, including a null provider only when it matches `auth.guards.api.provider`, and returns an error when none exists.

Confidential secrets use the application's existing asynchronous Eloquent hash service. The creating or rotating `Client` temporarily retains its setter input in `plainSecret`; fresh database reads do not. Default JSON output hides `secret` and does not append the plaintext. Copy a newly generated plaintext only when delivering it to its owner, and never log it. An `Arc<tokio::sync::Mutex<M>>` returned by `record.model::<M>()` refers to the actual selected model; `sameInstance` checks shared object identity, not whether two model rows have the same key.

Use `client.append(&["plain_secret"])` to explicitly append the current instance’s temporary plaintext. A freshly hydrated model appends null. Appended values still obey `makeHidden` and `setVisible`.

`ClientModel::hasScope` and `ClientRecord::hasScope` allow all scopes when the raw `scopes` attribute is absent or null. Otherwise, they check the configured array using PHP non-strict comparison. Parent-scope inheritance is disabled by default. Calling `passport.withInheritedScopes.store(true, std::sync::atomic::Ordering::Relaxed)` on the current application’s instance lets `orders` grant descendants such as `orders:read`. Here, `*` is an ordinary scope name. The switch belongs to each application instance. Call these methods within that application’s request or Artisan lifecycle.

Call `passport.useClientModel::<CustomClient>()` during application setup before resolving repositories. The compiled type implements `ClientModel` and Eloquent `Model`; its unique-ID validation, global scopes, events and casts remain in effect. Malformed UUID/ULID keys return no result before connection resolution. Repository queries and unbound `Client::query()`/`save()` use an explicit model connection first, then `passport.connection`, then the database default. Created and loaded models retain their actual connection, so later configuration changes do not redirect their saves. `Client::queryOn(&connection)` selects a connection explicitly. Transaction writes check the effective model connection and reject a transaction from another connection. Rust exposes `resolveConnectionName()` with an optionally owned name for application configuration and `getConnection()` for the actual connection. Secret assignment uses PHP scalar string coercion. Failed hashing preserves the stored hash and retains the current setter input in `plainSecret`. Mass assignment follows Rust’s existing attribute key order and preserves earlier successful assignments if a later setter fails.

## Client factories

`Client::factory()?` constructs `database::factories::ClientFactory<Client>`. Use `asPublic`, `asPasswordClient`, `asPersonalAccessTokenClient`, `asImplicitClient`, `asClientCredentials`, or `asDeviceCodeClient` to apply the corresponding grant state. Later states override earlier attributes. The default is a confidential authorization code client with a random 40-character secret and one generated redirect URI. Owners are null and revoked is false.

Call `make().await?` for an unsaved model or `create().await?` to persist it. Assignment uses the model’s asynchronous mutators and casts: the new Client retains `plainSecret`, while a database reload contains only its hash. `state` adds application attributes; `intoFactory()` exposes the existing Eloquent builder for counts, relationships and callbacks.

Rust selects a factory model at compile time: use `ClientFactory::<CustomClient>::new()?` for a custom `ClientModel`. This does not dynamically read `useClientModel`. Construction resolves and captures the model’s effective connection, including `passport.connection`, and returns configuration errors. Construct a fresh factory after changing that configuration. Only the authorization-code protocol is currently available.

## Client commands and revocation

After running the migrations, create clients with `./artisan passport:client`. Options `--personal`, `--password`, `--client`, `--implicit` and `--device` select the grant; without these flags the command creates an authorization code client. If several flags are present, that order determines precedence. `--name`, `--provider` and `--redirect_uri` supply prompt values; redirects are comma separated. `--public` omits the secret for password, device and authorization code clients. Implicit clients are always public; personal and client credentials clients are confidential.

With `--no-interaction`, the name defaults to `app.name`, the callback to the application URL plus `/auth/callback`, and the additional device flow to false. Interactive authorization code creation asks about device flow when `deviceCodeGrantEnabled` is true (the default). The command prints the client ID and, for confidential clients, the temporary secret and a reminder to retain it. Personal client creation prints only its success message.

`clients.delete(&client).await?` loads the client's access tokens in batches of 1000 and revokes each refresh token before its access token, then revokes the client. These are Eloquent saves with casts and events. Errors propagate and earlier saves remain; a save cancelled by an event returns false and does not stop the chain, matching Passport. Authorization codes are unchanged. `TokenModel::revoke` and `RefreshTokenModel::revoke` also expose individual revocation.

## Models and relationships

`Token`, `RefreshToken` and `AuthCode` use string keys without auto increment, boolean `revoked` and datetime `expires_at` casts, and the Passport connection fallback. Only `Token` uses timestamps; its scopes are an array. `Token::can` rejects empty scopes, recognizes the PHP loose `*` wildcard and uses the application inheritance setting; `cant` negates it.

`ClientModel` exposes `tokens`, `authCodes`, `owner` and the deprecated `user::<U>()`; `Token` exposes `client`, `refreshToken` and the deprecated `user::<U>("authentication_key")`; `RefreshToken` exposes `accessToken`, and `AuthCode` exposes `client`. The token user key must match the compiled provider model's `Authenticatable` identifier; the client user relation uses its primary key. Register polymorphic owners through the existing `owner().target::<U>(alias, primary_key)` builder.

Rust binds relation result types at compile time. Default helpers return the default concrete models; use `tokensFor::<T>`, `clientFor::<C>`, `refreshTokenFor::<R>` or `accessTokenFor::<T>` for custom models. `passport.useTokenModel::<T>()` and `useRefreshTokenModel::<R>()` select the actual models used by repository revocation, including their connections and events. These shared registrations also apply to already resolved repositories. The caller chooses the compiled user provider model instead of PHP's runtime class string.

`clientUuids` defaults to true. Set `passport.clientUuids.store(false, std::sync::atomic::Ordering::Relaxed)` before constructing clients when using an existing numeric auto increment client table. Each instance retains the strategy selected at construction. This does not change an existing schema; the published client migration creates UUID keys.

## Scope JSON

`Scope::toJson()` retains the `id`, `description` field order and PHP’s default Unicode and slash escaping. Use `toJsonWithOptions(ScopeJsonOptions)` for `JSON_HEX_TAG`, `JSON_HEX_AMP`, `JSON_HEX_APOS`, `JSON_HEX_QUOT`, `JSON_NUMERIC_CHECK`, `JSON_UNESCAPED_SLASHES`, `JSON_PRETTY_PRINT`, `JSON_UNESCAPED_UNICODE`, `JSON_PRESERVE_ZERO_FRACTION` and `JSON_UNESCAPED_LINE_TERMINATORS`. Combine constants with `|`; `fromBits` accepts PHP option bits. Flags for invalid UTF-8 have no effect on Rust’s valid UTF-8 strings.

```rust
use openrust_passport::{Scope, ScopeJsonOptions};
let scope = Scope::new("orders/read", "Read orders");
let json = scope.toJsonWithOptions(
    ScopeJsonOptions::JSON_UNESCAPED_UNICODE | ScopeJsonOptions::JSON_UNESCAPED_SLASHES,
)?;
```

## Authorization code grant {#authorization-code-grant}

The provider registers GET, POST and DELETE `/oauth/authorize` with the `web` middleware and session locking, plus throttled POST `/oauth/token`. Set `passport.path` to change the `oauth` prefix and `passport.middleware` to add middleware to all these routes. Call `passport.ignoreRoutes()` before the package boots to register your own routes. Browser authentication uses `passport.guard` (default `web`); configure the application's normal login route and SessionGuard user provider. Publish and run the migrations, set `APP_KEY`, and generate the RSA keys first.

During application setup, resolve `Passport` and call `passport.tokensCan(BTreeMap::from([("read".to_owned(), "Read data".to_owned())]))`, `passport.setDefaultScope(vec!["read".to_owned()])` if needed, and `passport.authorizationView(consent)`. These settings belong to the application. The callback below returns consent data for an application UI; render the client, scopes and approve/deny controls in that UI:

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

Clients must generate a fresh unpredictable `state` and check it on the callback before accepting a code or error. Confidential clients authenticate the token exchange with HTTP Basic credentials or body `client_id` and `client_secret`. Body credentials take precedence. Parameters come from the request body, not the query. Successful responses contain `token_type`, `expires_in`, `access_token` and, for clients allowing refresh, `refresh_token`, with `Cache-Control: no-store` and `Pragma: no-cache`. Access tokens use RS256. Authorization codes expire after ten minutes and are consumed atomically with token persistence; competing exchanges have one winner. Current access and refresh expirations are twelve calendar months; refresh-token redemption and expiry customization belong to the next lifecycle unit.

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

Rust stores a validated native request in Session and invokes a compiled view callback instead of PHP object serialization. Codes and refresh tokens use the existing application's authenticated encryption plus application context; their internal envelope is not PHP ciphertext compatible. Client/code locks and a single database transaction keep code consumption, access-token and refresh-token saves atomic. Custom token models participate through Eloquent casts, events and their actual connection; save cancellation or database failure rolls back issuance. This unit does not yet provide resource Guard, refresh redemption, device authorization, other grants or the combined API installer.
