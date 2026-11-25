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

These are lifecycle services and the generated password Actions. The complete account center remains in progress. Device-token login, recent confirmation and token management are described below. Registering only the response adapter does not enable the credential policy.

## Explicit device-token login {#client-tokens}

Cookie authentication remains the default for the web application and first-party SPA. JSON requests to `/login` do not automatically issue a personal token. An already registered account can explicitly request a device token:

```sh
curl -X POST http://localhost:8000/api/auth/token \
  -H 'Accept: application/json' -H 'Content-Type: application/json' \
  -d '{"email":"developer@example.test","password":"YOUR_PASSWORD","device_name":"phone","abilities":["account:read"]}'
```

```json
{
  "success": true,
  "code": "ok",
  "message": "Success",
  "data": {
    "token_type": "Bearer",
    "token": "TOKEN_SHOWN_ONCE",
    "expires_at": "2030-02-01T03:04:05Z",
    "user": {
      "id": 1,
      "name": "Developer",
      "email": "developer@example.test",
      "email_verified_at": null
    }
  }
}
```

Successful issuance uses HTTP 201 and `Cache-Control: no-store`. Securely save the credential: only its hash is stored, and later reads do not return plaintext. Send `Authorization: Bearer TOKEN_SHOWN_ONCE` to protected endpoints such as `/api/user`. Server-side and mobile token clients do not need a browser session. First-party requests still follow their stateful middleware and CSRF protections.

`device_name` is required and limited to 255 characters. Optional `abilities` is an array of at most 100 names. The server checks `user.token_abilities`; unknown or empty grants receive 422. When verification is enabled, unverified users receive only the intersection with `user.unverified_token_abilities`, which defaults to the account and token-management `abilities`. The effective `MustVerifyEmail` state is honored. Verification does not expand an existing token. Business routes still need application authorization and verification middleware.

Routes follow `API_PREFIX`. With `API_PREFIX=api/admin`, use `/api/admin/auth/token` and `/api/admin/auth/token/challenge`. Existing information and current-user routes remain. The generated Provider explicitly calls `openrust_fortify::starter::client::register_routes::<User>(app)`; standalone Fortify applications are unchanged.

An active configured factor changes the correct-password response to HTTP 200, without an access token:

```json
{
  "success": true,
  "code": "auth_challenge_required",
  "message": "Complete the additional authentication step.",
  "data": {
    "challenge": "OPAQUE_CHALLENGE",
    "methods": [
      "totp",
      "recovery_code"
    ],
    "expires_at": "2030-01-02T03:09:05Z"
  }
}
```

Complete POST `/api/auth/token/challenge` with the server challenge and the current code. Alternatively send `recovery_code` instead of code. Successful completion returns the same 201 token format. Challenges expire after five minutes, can be consumed once, retain the original device and approved `abilities`, and cannot authenticate resource requests. Invalid factors return 422; expired, consumed or invalidated challenges return 401. Login reuses its configured limiter; factor attempts are additionally limited per account across challenge identifiers.

```json
{
  "challenge": "OPAQUE_CHALLENGE",
  "recovery_code": "YOUR_RECOVERY_CODE"
}
```

Issuance locks and rechecks current user state. Credential/version changes, relevant account/factor changes or a different application encryption key invalidate pending authentication. Recovery-code authentication consumes the code under a database lock; the ordinary public replacement method keeps its native behavior. A failed or interrupted exchange can require starting login again, without leaving a reusable authorized challenge.

The native timestamped migration publisher adds `create_user_auth_challenges_table` on the User connection. Raw challenge keys and passwords are not stored. Multiple nodes share their database, application key, authentication policy and cache; each node reads its own application configuration.

```sh
./artisan auth:prune-challenges --hours=0
```

The command retains expired records for 24 hours by default; `--hours=0` removes already expired records immediately. Schedule it with the existing [scheduler](../digging-deeper/scheduling.md) alongside `sanctum:prune-expired`. Expired authentication is rejected before cleanup; repeated cleanup is safe.

Messages follow the application/request locale and fallback. Published `lang/{locale}/user.json` entries override compiled starter defaults. Upgrades fill missing English, Simplified Chinese and Traditional Chinese keys while preserving custom translations.

This section covers device login and TOTP/recovery challenges. Token-client registration, mail verification/recovery and the complete account center remain in progress; recent confirmation and token management are described below.

## Recent confirmation and personal tokens {#account-tokens}

These endpoints belong to the explicitly installed `user` kit. Prefix every API path with your configured `API_PREFIX`; the examples use `/api`. Ordinary JSON login continues to use Cookie sessions.

| Method and path | Behavior |
| --- | --- |
| `POST /api/auth/confirm` | Verify the `current` `user`'s password, or the application's configured confirmation callback. |
| `GET /api/user/tokens` | List only the authenticated `user`'s safe `token` metadata. Requires `tokens:manage` for PAT clients. |
| `POST /api/user/tokens` | Create a device `token` after recent confirmation; return201. |
| `DELETE /api/user/tokens/{`token`}` | Revoke only an owned `token` after fresh confirmation; return204. |
| `POST /api/auth/logout` | Revoke the PAT actually authenticating this request; return204. No recent `proof` is required. |

For a PAT client, confirm the password with the same Bearer `token` that will perform the sensitive operation:

```http
POST /api/auth/confirm
Authorization: Bearer YOUR_TOKEN
Content-Type: application/json
Accept: application/json

{"purpose":"tokens","password":"YOUR_PASSWORD"}
```

```json
{"success":true,"code":"ok","message":"Success","data":{"proof":"OPAQUE_SINGLE_USE_PROOF","expires_at":"2030-01-01T03:00:00Z"}}
```

Use the `proof` once, through `X-Auth-Confirmation` or the JSON `proof` field. Obtain a new `proof` before another creation or revocation:

```http
POST /api/user/tokens
Authorization: Bearer YOUR_TOKEN
X-Auth-Confirmation: OPAQUE_SINGLE_USE_PROOF
Content-Type: application/json
Accept: application/json

{"device_name":"tablet","abilities":["account:read"]}
```

The201 response uses the same `token_type`, `token`, `expires_at` and `user` fields as [device login](#client-tokens). Omitting `abilities` selects the intersection of the server policy and the current parent token. A failed operation may consume its proof; confirm again before retrying. A child `token` cannot exceed the server whitelist, the unverified-account policy or its `current` parent `token`'s `abilities`. Retain the plain `token` securely: it is returned once and never included in the `token` list.

`GET /api/user/tokens` returns `data` as an array of `id`, `name`, `abilities`, `created_at`, `last_used_at`, `expires_at` and `current`. It excludes hashes, credential stamps and internal owner attributes. Revoking another `user`'s ID returns404; the client cannot choose which credential `POST /api/auth/logout` logs out. Successful204 responses have an empty body. Authentication after revocation returns401, while other devices remain authenticated.

For first-party Cookie/SPA clients, retain the normal CSRF flow and send credentials. `POST /user/confirm-password` records confirmation in the `current` Session; native Passkey confirmation records the same state. Then `POST /api/user/tokens` with `device_name` and `abilities`. A separate opaque `proof` is not required for this browser mode. `POST /api/auth/confirm` also accepts a stateful Cookie request and returns201 without `data`. Another browser session does not inherit confirmation; missing/expired confirmation returns423 with localized code `password_confirmation_required`. A missing CSRF `token` remains419.

PAT proofs are hashed in the application's database, bound to the `user`, `current` PAT, purpose, credential version and effective factor/email state. `auth.password_timeout` is a positive number of seconds, default10800. Proofs expire without waiting for cleanup; wrong devices/purposes, tampering and replay return401. User and `token` row locks cover issuance/revocation. A late expiry or a cancelled model event rolls back the database mutation. Password callbacks may support an application-specific identity flow; an account without a local password is never automatically confirmed. Stateless Passkey reauthentication remains a separate pending adapter.

Password errors return422; five failed confirmations within a minute cause429 with `Retry-After`. Missing `token` capabilities return403. All displayed messages use the request locale. Proofs, passwords and `token` secrets are excluded from old input. Responses containing a new proof/token use `no-store`. The complete account center, avatar, device list and account deletion remain in progress.
