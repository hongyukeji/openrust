# Fortify

## Introduction

Fortify supplies authentication routes and application extension contracts without shipping a user interface. Its authentication API follows Fortify 1.43.0.

## Fortify and Sanctum

Fortify manages registration, session login and credential changes. Use [Sanctum](sanctum.md) for the API authentication capabilities documented there. Fortify uses the existing authentication, session, CSRF, hashing, validation and notification services.

## Installation

Add `fortify` to the application’s `[features].default` list in `Cargo.toml`, preserving existing features, then use the source launcher:

```sh
./artisan fortify:install
```

The command publishes `config/fortify.rs`, `app/actions/fortify/`, the application provider and the two-factor columns and Passkeys migrations. It registers their Rust modules, configuration and providers, and adds the default User capability. Existing published files are preserved. Unsupported source layouts fail with a manual registration diagnostic. Installation does not run migrations.

Edit the published configuration: enable `email-verification` for the default User; provide view callbacks or set `views` to `false`. Then run the migration; the source launcher rebuilds the application when needed:

```sh
./artisan migrate --no-interaction
```

The discovery provider `FortifyResourcesServiceProvider` makes installation resources available. The explicitly registered `openrust_fortify::FortifyServiceProvider` enables runtime authentication. The generated application provider connects actions, models and named limiters. Compiled applications require rebuilding after source changes.

## Configuration and views

The installed defaults use guard `web`, password broker `users`, username and email field `email`, lowercased usernames, prefix `""`, and home `/home`. These are application stub defaults; the package defaults differ. Set `fortify.features` to the needed feature names and `fortify-options.two-factor-authentication` to `{"confirm":true,"confirmPassword":true}`. The installer loads this separate Rust options group.

`fortify.views=false` removes UI routes while keeping enabled action endpoints. It does not disable session cookies or CSRF. Provide a frontend route named `password.reset`, or configure native reset notification URL generation, so reset emails can be built. View callbacks include `login_view`, `register_view`, `request_password_reset_link_view`, `reset_password_view`, `verify_email_view`, `confirm_password_view` and `two_factor_challenge_view`.

## Authentication

`POST /login` accepts the configured username, `password` and optional `remember`. Successful ordinary JSON login returns `200` with `data.two_factor` set to `false`. A pending two-factor challenge sets it to `true`. Successful authentication rotates the session. `POST /logout` invalidates it and regenerates the CSRF token. Send the session cookie and CSRF header on state-changing browser requests. JSON validation failures return `422`; named throttling returns `429`.

Built-in business JSON responses use [`ApiResult`](../the-basics/responses.md#business-responses). Password confirmation status is available at `data.confirmed`, and two-factor QR, secret and recovery code responses place their payload under `data`. Successful actions without payload omit `data` and retain their HTTP status. Validation errors include `errors`, and HTTP 204 responses have an empty body. Browser redirects, custom response contracts and session Passkeys retain their existing response behavior.

Use `Fortify::authenticate_using` for authentication lookup, `confirm_passwords_using` for password confirmation, and `authenticate_through` to replace the login pipeline. The default pipeline handles throttling, username canonicalization, two-factor redirection, authentication and session preparation. Configure `fortify.redirects` for web outcomes.

The installed `login` limiter allows five requests per minute per transliterated lowercase username and IP. It uses `RateLimiter::for_async` to read the real request input. A configured named limiter replaces Fortify’s built-in failed-attempt limiter. Applications may return `Limit`, multiple limits, `Unlimited` or a response from a named policy.

## Two-factor authentication

After password confirmation, `POST /user/two-factor-authentication` creates an encrypted TOTP secret and eight recovery codes. Read the QR SVG from `GET /user/two-factor-qr-code`, the secret from `GET /user/two-factor-secret-key`, and submit a TOTP `code` to `POST /user/confirmed-two-factor-authentication` when confirmation is enabled.

`GET /user/two-factor-recovery-codes` lists codes; `POST` on the same path regenerates them. `DELETE /user/two-factor-authentication` disables the capability. The default model hides encrypted secrets and codes from serialization.

After a challenged login, submit `code` or `recovery_code` to `POST /two-factor-challenge` using the same session. Recovery code redemption replaces that code. Challenge state, replay checks, named throttling and the native authentication guard remain part of the request flow. Applications can replace the `TwoFactorAuthenticationProvider` contract.

`TwoFactorAuthenticatable::replace_recovery_code` remains the overridable model entry point. Its default implementation uses the configured Fortify recovery-code generator and encrypter, saves the encrypted model attribute, then dispatches `RecoveryCodeReplaced` with the original code and model context. Database errors propagate through the existing result; custom implementations retain control of the model method.

## Registration and account updates

`POST /register` uses `CreatesNewUsers`; the installed `CreateNewUser` validates name and email, checks email uniqueness against the User model, validates the default password and confirmation, hashes it and saves the account. JSON success returns `201`.

`PUT /user/profile-information` uses `UpdatesUserProfileInformation`. An email change clears verification and sends a new native notification. `PUT /user/password` uses `UpdatesUserPasswords`, verifies `current_password:web`, hashes the new password, removes reset tokens and emits the update event. The installed `update_from_request` reads the real `web` guard independently of the user being updated. Calling that default action without a request fails explicitly; custom actions retain the original `update` method.

Use `create_users_using`, `update_user_profile_information_using`, `update_user_passwords_using` and `reset_user_passwords_using` on the application’s Fortify instance to replace actions. The installed password rules are `required`, `string`, `Password::default()` and `confirmed`. Profile and password web validation use named error bags.

## Password reset

`POST /forgot-password` accepts the configured email field and sends a native password reset notification through the selected broker. Submit `token`, email, `password` and `password_confirmation` to `POST /reset-password`. The installed `ResetsUserPasswords` action validates, hashes and saves the actual broker user. Completion rotates the remember token and emits `PasswordReset`; it does not log the user in. Invalid action input leaves the reset claim available for retry, while a successfully consumed token cannot be reused.

Custom `ResetsUserPasswords` actions validate and save the password on the actual broker user. Fortify then persists remember-token rotation and dispatches `PasswordReset`; keep this completion step separate from the custom action to avoid repeating those effects.

JSON reset outcomes include a translated message; failures use validation errors. Web outcomes use redirects and session status. Implement the password response contracts to replace them; their Rust `to_response` method receives the broker status explicitly.

## Email verification

Enable `email-verification` for users implementing `MustVerifyEmail`. Registration and changed profile emails use the native verification notification. `POST /email/verification-notification` resends it. The signed `GET /email/verify/{id}/{hash}` link validates the current user and current email, updates verification and emits the verification event. Use the existing `verified` middleware to protect application routes. Configure a real mail transport for delivery; the default `log` mailer records messages.

A `MustVerifyEmail` implementation reports whether the persisted verification state actually changed. Fortify dispatches `Verified` for that transition and then updates the session user; repeated verification does not emit the event again. Keep event dispatch in the verification flow when implementing the model capability.

## Password confirmation

`POST /user/confirm-password` verifies `password` and records the confirmation time. `GET /user/confirmed-password-status` returns `{"confirmed":true}` while it remains recent. Credential management uses `password.confirm` when configured. Applications may replace response contracts such as `LoginResponse`, `RegisterResponse`, `PasswordUpdateResponse` and two-factor responses through the existing container; each application owns its replacements.

## Passkeys

The Passkeys API works with the `@laravel/passkeys` 0.4.1 client. `fortify:install` publishes the `passkeys` table migration, adds `PasskeyAuthenticatable` to the default User, and registers `Passkeys::use_user_model::<User>()` in the generated provider. Keep `passkeys` in `fortify.features`, rebuild and migrate. Custom User models implement `openrust_fortify::passkeys::PasskeyAuthenticatable` and require explicit registration in their application provider.

Set `fortify.passkeys.relying_party_id`, `allowed_origins`, `user_handle_secret` and `timeout`. Defaults derive the RP ID and exact origin from `APP_URL`, use `PASSKEYS_USER_HANDLE_SECRET` with `APP_KEY` fallback, and set a 60000 ms timeout. Production origins require HTTPS; HTTP localhost is supported for development. Origins include their port and exclude trailing slashes and paths. Keep the user handle secret stable when rotating the application key: handles are 64-byte hexadecimal HMAC-SHA256 values derived from the model table and key. Changing that secret changes handles for new registrations.

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/user/passkeys/options` | Registration options for the signed-in user |
| POST | `/user/passkeys` | Validate and store `name` and `credential` |
| DELETE | `/user/passkeys/{passkey}` | Delete an owned credential |
| GET | `/passkeys/login/options` | Discoverable login options |
| POST | `/passkeys/login` | Verify `credential`, optional `remember`, and sign in |
| GET | `/passkeys/confirm/options` | Options restricted to the current account |
| POST | `/passkeys/confirm` | Verify and record password confirmation |

Routes inherit Fortify's configured prefix, domain, middleware and guard. Registration and deletion require recent password confirmation by default; `fortify-options.passkeys.confirm_password=false` disables that management gate. The generated `passkeys` named limiter allows ten requests per minute per session for options and per credential ID for submissions. The package adds no GET listing route: use the native `user.passkeys().get_results(&user).await?` relationship in your authenticated application controller, returning only fields your interface needs.

The fixed official client performs real WebAuthn operations:

```js
import { Passkeys } from '@laravel/passkeys';
await Passkeys.register({ name: 'My device' });
await Passkeys.verify({ remember: true });
await Passkeys.verify({ routes: {
  options: '/passkeys/confirm/options', submit: '/passkeys/confirm'
} });
```

Supply a `csrf-token` meta tag and the session cookie. Override the client routes when using a prefix. Registration JSON contains `status`, `id` and `name`; verification JSON contains `redirect`. Web requests redirect and flash registration/deletion status. The four response contracts in `passkeys::contracts` may be replaced through the native container. `Passkeys::authorize_login_using` supplies an application-local async authorization hook after cryptographic verification. Native `PasskeyRegistered`, `PasskeyVerified` and `PasskeyDeleted` events carry the user and persisted model context.

Registration requires a resident credential and user verification, with ES256/RS256 and none attestation. Server-side Session state binds the challenge to the operation and account, with an enforced deadline. Atomic cache claims reject concurrent reuse; credential updates use a MySQL row lock to reject counter rollback across independent challenges. Multiple application processes must share both Session storage and a cache supporting atomic `add`; the array drivers cover one process. Discoverable login also checks the stored user handle. Losing the server-side state requires restarting the ceremony.

Select an actual Eloquent model with `Passkeys::use_passkey_model::<CustomPasskey>()` in the application provider. Implement `PasskeyModel` on a `Model + Clone` type with the credential casts and columns; its table, primary key, accessors and observers apply to queries, inserts, row-locked verification and deletion. `Passkeys::passkey_model()` and `user_model()` expose the selected type names. Rust uses `PasskeyRelation` for `get_results`, `exists` and `create`, returning `PasskeyRecord` values. Record methods such as `get_key`, `get_attribute_value` and `to_array` are asynchronous. `record.model::<CustomPasskey>()` returns the shared actual model behind an async mutex. Release that lock before calling another record method. Event `passkey::<CustomPasskey>(context)` reads a typed snapshot; the event's `passkey` record retains the actual shared model.

Keep credential verification in the existing Passkeys action flow. `PasskeyRecord` retains the selected model instance, and verification and credential updates remain protected by the same model lock and database transaction. Custom model casts and observers continue to apply; application code does not need to verify and save the credential separately.

The default model appends `authenticator`, resolved from the registration AAGUID. `Aaguids::label_for`, `icon_for(id, "light")`, `all` and `flush` use the pinned upstream resources. Add `authenticator_icon_light` and `authenticator_icon_dark` to model appends only when needed. Unknown or absent AAGUIDs return null; credentials stored before this field existed remain usable. With none attestation, these names and icons describe the reported AAGUID and do not certify the manufacturer.

For standalone resource publishing, explicitly register `PasskeysResourcesServiceProvider` from `openrust_passkeys` in your provider list and rebuild. Then run:

```sh
cargo run -- artisan vendor:publish --tag=passkeys-config
cargo run -- artisan vendor:publish --tag=passkeys-migrations
```

Publishing preserves customized files and avoids duplicate migrations. Wire the generated Rust configuration and migration modules into the application's loader and migration list before rebuilding and migrating. The resource provider only registers publishable files; standalone authentication additionally requires the configured `PasskeysServiceProvider`, user model, guard and middleware. Fortify installations continue to use their generated Fortify configuration.

PHP credential JSON is not a portable Rust credential storage format.

The installer supports the default model’s hidden-field list written with `User::PASSWORD` and `User::REMEMBER_TOKEN`, as well as the equivalent string literals. It preserves custom source and requests manual configuration for unrecognized layouts.

## Transactional passkey cleanup

```rust
passkeys.purge_user_in_transaction(transaction, user).await?;
```

Hold the user row lock and use its connection transaction. This physically deletes the registered user’s credentials, including scoped or soft-deleted records, with model force-delete events. Roll back on any error. The persisted, unchanged primary key and registered user type must match. A missing optional table returns zero; an existing table on another connection is rejected. This Rust transaction extension leaves ordinary passkey APIs unchanged.

## Transactional passkey actions {#transactional-passkey-actions}

```rust
use openrust_fortify::passkeys::actions::{StorePasskey, VerifyPasskey, DeletePasskey};

let record = StorePasskey(passkeys.clone())
    .call_in_transaction(user, transaction, "My device", credential, &registration_options)
    .await?;
let record = VerifyPasskey(passkeys.clone())
    .call_in_transaction(user, transaction, assertion, &verification_options)
    .await?;
DeletePasskey.call_in_transaction(user, transaction, &record).await?;
```

These are separate ceremony examples. Hold the persisted user row lock, use its connection transaction, and commit or roll back in the calling operation. Discard or refresh returned models after rollback. The selected passkey model must use the same connection. Model mutators, casts, observers, actual primary keys and native registered/verified/deleted events remain active. Cancelled saves, cancelled deletes and a model claiming success without persisting verification or deletion return errors. Events run before the caller commits, matching the native actions; schedule external side effects after commit.

For discoverable login, use `VerifyPasskey::call_discoverable_in_transaction` with the locked user. It still validates the authenticator's `userHandle`; resolving the account first does not waive that requirement. `VerifyPasskey::get_passkey(&credential)` and `passkeys.user_for(&record).await?` can resolve the selected credential and registered user before locking. These reads do not authenticate anyone: complete signature, origin, RP, user-verification and counter checks through the action before creating a Session or token. Ordinary `call` methods remain available; the default verifier reads a same-connection user inside its credential transaction, including on SQLite.

Keep `RegistrationOptions` and `AuthenticationOptions` state on the server. Return only `options()` to the client and enforce expiry and single use through the existing HTTP ceremony or your authenticated controller. A transaction Action alone does not provide challenge storage, client authorization or recent confirmation. The user starter's optional management and passwordless flow are tracked separately from these native primitives.

## Transactional two-factor actions {#transactional-two-factor-actions}

```rust
openrust_fortify::actions::EnableTwoFactorAuthentication::new(provider, fortify)
    .call_in_transaction(user, transaction, false)
    .await?;
```

EnableTwoFactorAuthentication, ConfirmTwoFactorAuthentication, DisableTwoFactorAuthentication and GenerateNewRecoveryCodes also provide `call_in_transaction`. Reuse an existing user lock and pass the user connection transaction. The example assumes that lock is already held. The caller must roll back the whole operation on error and discard or reload its in-memory model after rollback. A cancelled save is an error; even a no-op rejects the wrong connection. Ordinary `call` behavior remains unchanged. Generation, encryption, verification and events use the same implementation. Events fire after saving and before the caller commits; listeners with external effects must use the existing after-commit facilities. A provider may already have consumed a verification code when a later database operation fails, so retry with a fresh code.

### Sensitive authentication diagnostics

The user starter kit marks its authentication and account routes with the `sensitive` middleware. Telescope retains only the HTTP method, status, duration and route metadata for these requests. It omits request and response bodies, headers, query strings and diagnostic entries such as SQL bindings, mail contents and model attributes. Request-origin log metadata uses the route template instead of a reset token in the URL. Ordinary routes retain their existing recording behavior.

For a custom authentication route, add `.middleware_name("sensitive")` to the route or its `RouteGroup`. The kernel identifies prepared protected routes before global middleware and request events; custom diagnostic integrations must honor `ProtectSensitiveData::current_path()` or the marker in `RequestAttributes`. This does not redact a secret that application code explicitly sends to an unrelated external logger.

Encrypted queued notifications automatically carry a non-secret flag through the existing hidden Context metadata. The native Worker restores it for delivery and failure events, so Telescope skips the decrypted mail contents and associated diagnostics. The next ordinary job uses a fresh scope and is recorded normally. This also protects previously queued encrypted notifications when their handler runs; it does not encrypt an ordinary notification automatically.

For a custom sensitive operation, reuse `Context::scope`:

```rust
use openrust::{log::context::SENSITIVE_DATA, support::facades::Context};

Context::scope(
    async {
        // Run the sensitive application operation here.
    },
    Default::default(),
    [(SENSITIVE_DATA.to_owned(), true.into())]
        .into_iter()
        .collect(),
)
.await?;
```

The reserved key is `openrust::log::context::SENSITIVE_DATA`. Context restores its previous value on success, error or cancellation. Telescope keeps the current collector protected until that request or job ends; it does not resume recording private data after an inner scope exits. Custom diagnostic integrations must also honor this hidden flag.
