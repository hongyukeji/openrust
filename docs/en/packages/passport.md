# Passport

## Installation {#installation}

Passport's configuration, key lifecycle and five publishable OAuth migrations are available. OAuth endpoints and token guards are delivered in the following Passport units; `install:api --passport` is not available yet. Existing Sanctum installation remains independent.

The current application has an optional `passport` Cargo feature. Add it to Cargo's default features to keep it enabled for `./artisan` and normal builds, preserving any existing defaults:

```toml
[features]
default = ["passport"]
```

Package discovery registers `openrust_passport::PassportServiceProvider`. Applications without discovery can register this provider with `withProviders`. Enabling the provider does not create tables, generate keys, or expose placeholder OAuth routes.

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

The first client repository batch supports lookup, creation, update, secret rotation and owner filtering using real Eloquent models. The `passport:client` command, revoking clients and their tokens, model relationships and complete scope configuration are still pending; this does not enable OAuth endpoints or token authentication.

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

Call `passport.useClientModel::<CustomClient>()` during application setup before resolving repositories. The compiled type implements `ClientModel` and Eloquent `Model`; its unique-ID validation, global scopes, events and casts remain in effect. Malformed UUID/ULID keys return no result before connection resolution. Repository queries use an explicit model connection first, then `passport.connection`, then the database default. Created and loaded models retain their connection for subsequent saves. Unbound `Client::query()`/`save()` connection fallback, the complete Client model surface, `plain_secret` appending, PHP JSON encoding options and exact float-to-string coercion are not part of this accepted batch. Use the repository or an explicitly bound `Client::queryOn(&connection)` for the documented operations.
