# Fortify

## Introduction

Fortify supplies authentication routes and application extension contracts without shipping a user interface. This page covers the verified API-F01 authentication and API-F02 Passkeys lifecycle against Fortify 1.43.0.

## Fortify and Sanctum

Fortify manages registration, session login and credential changes. Use [Sanctum](sanctum.md) for the API authentication capabilities documented there. Fortify uses the existing authentication, session, CSRF, hashing, validation and notification services.

## Installation

Enable the `fortify` Cargo feature and run the compiled command:

```sh
cargo +1.94.0 build --locked --features fortify --bin main
./target/debug/main artisan fortify:install
```

The command publishes `config/fortify.rs`, `app/actions/fortify/`, the application provider and the two-factor columns and Passkeys migrations. It registers their Rust modules, configuration and providers, and adds the default User capability. Existing published files are preserved. Unsupported source layouts fail with a manual registration diagnostic. Installation does not run migrations.

Edit the published configuration: enable `email-verification` for the default User; provide view callbacks or set `views` to `false`. Then rebuild and migrate:

```sh
cargo +1.94.0 build --locked --features fortify --bin main
./target/debug/main artisan migrate --no-interaction
```

The discovery provider `FortifyResourcesServiceProvider` makes installation resources available. The explicitly registered `openrust_fortify::FortifyServiceProvider` enables runtime authentication. The generated application provider connects actions, models and named limiters. Compiled applications require rebuilding after source changes.

## Configuration and views

The installed defaults use guard `web`, password broker `users`, username and email field `email`, lowercased usernames, prefix `""`, and home `/home`. These are application stub defaults; the package defaults differ. Set `fortify.features` to the needed feature names and `fortify-options.two-factor-authentication` to `{"confirm":true,"confirmPassword":true}`. The installer loads this separate Rust options group.

`fortify.views=false` removes UI routes while keeping enabled action endpoints. It does not disable session cookies or CSRF. Provide a frontend route named `password.reset`, or configure native reset notification URL generation, so reset emails can be built. View callbacks include `loginView`, `registerView`, `requestPasswordResetLinkView`, `resetPasswordView`, `verifyEmailView`, `confirmPasswordView` and `twoFactorChallengeView`.

## Authentication

`POST /login` accepts the configured username, `password` and optional `remember`. Successful ordinary JSON login returns `200` with `two_factor:false`; a pending two-factor challenge returns `two_factor:true`. Successful authentication rotates the session. `POST /logout` invalidates it and regenerates the CSRF token. Send the session cookie and CSRF header on state-changing browser requests. JSON validation failures return `422`; named throttling returns `429`.

Use `Fortify::authenticateUsing` for authentication lookup, `confirmPasswordsUsing` for password confirmation, and `authenticateThrough` to replace the login pipeline. The default pipeline handles throttling, username canonicalization, two-factor redirection, authentication and session preparation. Configure `fortify.redirects` for web outcomes.

The installed `login` limiter allows five requests per minute per transliterated lowercase username and IP. It uses `RateLimiter::forAsync` to read the real request input. A configured named limiter replaces Fortify’s built-in failed-attempt limiter. Applications may return `Limit`, multiple limits, `Unlimited` or a response from a named policy.

## Two-factor authentication

After password confirmation, `POST /user/two-factor-authentication` creates an encrypted TOTP secret and eight recovery codes. Read the QR SVG from `GET /user/two-factor-qr-code`, the secret from `GET /user/two-factor-secret-key`, and submit a TOTP `code` to `POST /user/confirmed-two-factor-authentication` when confirmation is enabled.

`GET /user/two-factor-recovery-codes` lists codes; `POST` on the same path regenerates them. `DELETE /user/two-factor-authentication` disables the capability. The default model hides encrypted secrets and codes from serialization.

After a challenged login, submit `code` or `recovery_code` to `POST /two-factor-challenge` using the same session. Recovery code redemption replaces that code. Challenge state, replay checks, named throttling and the native authentication guard remain part of the request flow. Applications can replace the `TwoFactorAuthenticationProvider` contract.

## Registration and account updates

`POST /register` uses `CreatesNewUsers`; the installed `CreateNewUser` validates name and email, checks email uniqueness against the User model, validates the default password and confirmation, hashes it and saves the account. JSON success returns `201`.

`PUT /user/profile-information` uses `UpdatesUserProfileInformation`. An email change clears verification and sends a new native notification. `PUT /user/password` uses `UpdatesUserPasswords`, verifies `current_password:web`, hashes the new password, removes reset tokens and emits the update event. The installed `updateFromRequest` reads the real `web` guard independently of the user being updated. Calling that default action without a request fails explicitly; custom actions retain the original `update` method.

Use `createUsersUsing`, `updateUserProfileInformationUsing`, `updateUserPasswordsUsing` and `resetUserPasswordsUsing` on the application’s Fortify instance to replace actions. The installed password rules are `required`, `string`, `Password::default()` and `confirmed`. Profile and password web validation use named error bags.

## Password reset

`POST /forgot-password` accepts the configured email field and sends a native password reset notification through the selected broker. Submit `token`, email, `password` and `password_confirmation` to `POST /reset-password`. The installed `ResetsUserPasswords` action validates, hashes and saves the actual broker user. Completion rotates the remember token and emits `PasswordReset`; it does not log the user in. Invalid action input leaves the reset claim available for retry, while a successfully consumed token cannot be reused.

JSON reset outcomes include a translated message; failures use validation errors. Web outcomes use redirects and session status. Implement the password response contracts to replace them; their Rust `toResponse` method receives the broker status explicitly.

## Email verification

Enable `email-verification` for users implementing `MustVerifyEmail`. Registration and changed profile emails use the native verification notification. `POST /email/verification-notification` resends it. The signed `GET /email/verify/{id}/{hash}` link validates the current user and current email, updates verification and emits the verification event. Use the existing `verified` middleware to protect application routes. Configure a real mail transport for delivery; the default `log` mailer records messages.

## Password confirmation

`POST /user/confirm-password` verifies `password` and records the confirmation time. `GET /user/confirmed-password-status` returns `{"confirmed":true}` while it remains recent. Credential management uses `password.confirm` when configured. Applications may replace response contracts such as `LoginResponse`, `RegisterResponse`, `PasswordUpdateResponse` and two-factor responses through the existing container; each application owns its replacements.

## Passkeys

The API-F02 lifecycle uses Laravel Passkeys server 0.3.0 and the tested `@laravel/passkeys` 0.4.1 client. `fortify:install` publishes the `passkeys` table migration, adds `PasskeyAuthenticatable` to the default User, and registers `Passkeys::useUserModel::<User>()` in the generated provider. Keep `passkeys` in `fortify.features`, rebuild and migrate. Custom User models implement `openrust_fortify::passkeys::PasskeyAuthenticatable` and require explicit registration in their application provider.

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

Routes inherit Fortify's configured prefix, domain, middleware and guard. Registration and deletion require recent password confirmation by default; `fortify-options.passkeys.confirmPassword=false` disables that management gate. The generated `passkeys` named limiter allows ten requests per minute per session for options and per credential ID for submissions. The package adds no GET listing route: use the native `user.passkeys().getResults(&user).await?` relationship in your authenticated application controller, returning only fields your interface needs.

The fixed official client performs real WebAuthn operations:

```js
import { Passkeys } from '@laravel/passkeys';
await Passkeys.register({ name: 'My device' });
await Passkeys.verify({ remember: true });
await Passkeys.verify({ routes: {
  options: '/passkeys/confirm/options', submit: '/passkeys/confirm'
} });
```

Supply a `csrf-token` meta tag and the session cookie. Override the client routes when using a prefix. Registration JSON contains `status`, `id` and `name`; verification JSON contains `redirect`. Web requests redirect and flash registration/deletion status. The four response contracts in `passkeys::contracts` may be replaced through the native container. `Passkeys::authorizeLoginUsing` supplies an application-local async authorization hook after cryptographic verification. Native `PasskeyRegistered`, `PasskeyVerified` and `PasskeyDeleted` events carry the user and persisted model context.

Registration requires a resident credential and user verification, with ES256/RS256 and none attestation. Server-side Session state binds the challenge to the operation and account, with an enforced deadline. Atomic cache claims reject concurrent reuse; credential updates use a MySQL row lock to reject counter rollback across independent challenges. Multiple application processes must share both Session storage and a cache supporting atomic `add`; the array drivers cover one process. Discoverable login also checks the stored user handle. Losing the server-side state requires restarting the ceremony.

Select an actual Eloquent model with `Passkeys::usePasskeyModel::<CustomPasskey>()` in the application provider. Implement `PasskeyModel` on a `Model + Clone` type with the credential casts and columns; its table, primary key, accessors and observers apply to queries, inserts, row-locked verification and deletion. `Passkeys::passkeyModel()` and `userModel()` expose the selected type names. Rust uses `PasskeyRelation` for `getResults`, `exists` and `create`, returning `PasskeyRecord` values. Record methods such as `getKey`, `getAttributeValue` and `toArray` are asynchronous. `record.model::<CustomPasskey>()` returns the shared actual model behind an async mutex. Release that lock before calling another record method. Event `passkey::<CustomPasskey>(context)` reads a typed snapshot; the event's `passkey` record retains the actual shared model.

The default model appends `authenticator`, resolved from the registration AAGUID. `Aaguids::labelFor`, `iconFor(id, "light")`, `all` and `flush` use the pinned upstream resources. Add `authenticator_icon_light` and `authenticator_icon_dark` to model appends only when needed. Unknown or absent AAGUIDs return null; credentials stored before this field existed remain usable. With none attestation, these names and icons describe the reported AAGUID and do not certify the manufacturer.

For standalone resource publishing, explicitly register `PasskeysResourcesServiceProvider` from `openrust_passkeys` in your provider list and rebuild. Then run:

```sh
cargo run -- artisan vendor:publish --tag=passkeys-config
cargo run -- artisan vendor:publish --tag=passkeys-migrations
```

Publishing preserves customized files and avoids duplicate migrations. Wire the generated Rust configuration and migration modules into the application's loader and migration list before rebuilding and migrating. The resource provider only registers publishable files; standalone authentication additionally requires the configured `PasskeysServiceProvider`, user model, guard and middleware. Fortify installations continue to use their generated Fortify configuration.

PHP credential JSON is not a portable Rust credential storage format.
