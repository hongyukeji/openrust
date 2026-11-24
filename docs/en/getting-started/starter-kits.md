# Starter Kits

## Support status {#support-status}

The explicit user starter preset installs native Fortify actions, safe user responses and basic Blade authentication pages. The full account center, personal token login protocol and optional-factor UI are still under development; these are not part of the completed preset described here. React, Vue and Livewire frontends are not generated.

## Available entry points {#available-entry-points}

```shell
./artisan init --starter-kit=full --extensions=api,fortify
# Headless APIs, with minimal browser pages for email links:
./artisan init --starter-kit=api --extensions=api,fortify
```

Run from a configured application with its database and mail settings. The command publishes resources, registers providers, rebuilds and offers migrations. In automation use `--no-interaction` after configuring the environment. `all` does not implicitly install the user starter. Choose one preset.

Both presets enable first-party stateful API authentication. The full preset adds login, registration and account pages; the API preset keeps only email-link landing pages. API paths follow `config/api.rs` and `API_PREFIX`: the protected user endpoint defaults to `/api/user`, or `/api/admin/user` with `API_PREFIX=api/admin`. The public API information route is preserved.

For Cookie clients, obtain CSRF cookies and send the latest `X-XSRF-TOKEN`, including after authentication rotates the session. JSON requests alone do not issue personal access tokens. See [Sanctum](../packages/sanctum.md) and [Fortify](../packages/fortify.md).

## Configuration and existing applications {#scope}

Generated `config/user.rs` controls `USER_STARTER_PROFILE`, registration, email verification and optional factors. Registration and verification default to enabled; additional factors default to disabled. The starter preserves native guards and prefixes. Configure a working mail transport before relying on verification or password recovery.

Repeated installation preserves existing published files. `--force` explicitly replaces the selected generated resources; review application changes first. Custom provider lists, bootstrap middleware or API user handlers that cannot be safely recognized require manual wiring instead of silent replacement. Business routes outside the stock user handler are preserved.

## Optional JSON responses {#json-responses}

The generated application provider installs `openrust_fortify::starter::StarterKitServiceProvider`. Existing Fortify applications may explicitly register this response adapter without installing pages:

```rust
Box::new(openrust_fortify::starter::StarterKitServiceProvider),
```

HTML requests retain native redirects and flash messages. JSON login, registration and completed two-factor login return the current user through `ApiResponse`; registration uses 201. Updates omit empty data; logout and verification retain empty 204 bodies. Messages follow the application locale. Successful authentication responses use `Cache-Control: no-store`.

`UserResource` reads only `id`, `name`, `email` and `email_verified_at`, preserving string identifiers without enumerating attributes or credentials. Replace the existing Fortify response binding for a custom resource. The response adapter alone does not issue tokens or alter guards.

## Mail links and password-backed sessions {#mail-and-sessions}

The preset uses Fortify's `POST /forgot-password`, `POST /reset-password`, `PUT /user/profile-information` and `PUT /user/password` endpoints. Send JSON with `Accept: application/json`; keep the current CSRF cookie for Cookie authentication. Successful updates return 200; invalid fields or reset credentials return 422. Verification links require the authenticated owner, the current email hash, a valid signature and an unexpired timestamp. Successful JSON verification returns an empty 204; repeated verification is idempotent. Changing the email clears verification and sends a link for the new address.

The API preset also has minimal browser login, reset and verification pages. When a logged-out browser opens a verification link, login preserves the original signed URL. The receipt reads the stored verification state: adding `?verified=1` cannot verify an account. Configure the mail transport before relying on delivery. Setting `USER_EMAIL_VERIFICATION=false` disables verification routes and these verification emails while leaving password recovery available.

For the installed starter, login and valid remember-cookie recovery immediately record the existing password fingerprint. The Guard rejects a missing or outdated fingerprint on subsequent authentication reads. Password changes preserve the current browser session and reject other password-backed sessions; reset also rotates the remember token. Standalone authentication keeps its existing defaults.

## Credential lifecycle {#credential-lifecycle}

The explicit starter provides two additional migration templates (publication uses Laravel-style timestamped names): `0001_01_01_000020_add_credential_version_to_users_table.rs` and `0001_01_01_000021_add_credential_stamp_to_personal_access_tokens_table.rs`. They follow the configured user and token model tables/connections. Run the normal migrations before serving the upgraded application. Existing sessions must authenticate again and existing personal tokens must be reissued when enabling this policy. Do not manually reset the credential version.

The generated User retains Sanctum's `create_token` entry point. Call it inside an authenticated application scope, after required recent authentication and any additional factor. This service does not perform those HTTP checks for you:

```rust
use openrust::sanctum::HasApiTokens;

// `user` is the authenticated application User; authenticate before issuing.
let token = user.create_token(
    &sanctum,
    &database,
    "phone",
    &["account:read"],
    None,
).await?;
let plaintext = token.plain_text_token;
```

`user.token_expiration_days` defaults to 30. A supplied shorter expiry is retained; a longer expiry is capped. Sanctum's global expiry can reject a token earlier. `user.token_abilities` is the server-side whitelist; unknown or empty abilities are rejected. Plaintext is returned once; only the hash and a hidden credential stamp are stored. Keep secrets out of logs. Continue using `sanctum:prune-expired` for expired records; authentication rejects expiry before cleanup.

The generated password Action increments a durable user credential version in the same transaction as the password change. It preserves the current browser login while invalidating other sessions and personal tokens. Password reset invalidates all previous logins. Ordinary password rehashing and native logout of other browser sessions do not revoke personal tokens. A custom Action must call `openrust_fortify::starter::credentials::change_password`; direct database password writes do not advance this policy automatically. `credentials::revoke_all` also handles accounts without a local password; its caller must first obtain valid recent authentication.

User mutation and token issuance lock and recheck the current identity/version before committing. They require the same database connection identity; a separately configured token connection fails explicitly rather than claiming an atomic security change. Custom user models implement `Clone` and retain `credential_version`. A custom token model retains `credential_stamp` and hides it from serialization. Models with transient fields can override `PersonalAccessTokenModel::clone_token_model` to return `self.clone()` so attribute accessors retain that state.

These are lifecycle services and the generated password Actions. Device-token login/challenge endpoints and the complete account center are still under development. Registering only the response adapter does not enable the credential policy.
