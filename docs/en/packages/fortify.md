# Fortify

## Introduction

Fortify supplies authentication routes and application extension contracts without shipping a user interface. This page covers the verified API-F01 implementation against Fortify 1.43.0. Passkeys and their migration remain pending API-F02; remove `passkeys` from the published feature list before booting the runtime provider.

## Fortify and Sanctum

Fortify manages registration, session login and credential changes. Use [Sanctum](sanctum.md) for the API authentication capabilities documented there. Fortify uses the existing authentication, session, CSRF, hashing, validation and notification services.

## Installation

Enable the `fortify` Cargo feature and run the compiled command:

```sh
cargo +1.94.0 build --locked --features fortify --bin main
./target/debug/main artisan fortify:install
```

The command publishes `config/fortify.rs`, `app/actions/fortify/`, the application provider and the two-factor columns migration. It registers their Rust modules, configuration and providers, and adds the default User capability. Existing published files are preserved. Unsupported source layouts fail with a manual registration diagnostic. Installation does not run migrations.

Edit the published configuration: remove `passkeys`; enable `email-verification` for the default User; provide view callbacks or set `views` to `false`. Then rebuild and migrate:

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
