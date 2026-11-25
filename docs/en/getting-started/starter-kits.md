# Starter Kits

## Support status {#support-status}

The explicit user starter installs native Fortify actions, safe resources, Blade authentication and Cookie/token APIs. The sections below describe the available operations. Account deletion is available; optional-factor management pages are not provided. React, Vue and Livewire frontends are not generated.

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

`UserResource` reads only `id`, `name`, `email`, `email_verified_at` and nullable `avatar_url`, preserving string identifiers without enumerating attributes or credentials. Replace the existing Fortify response binding for a custom resource. The response adapter alone does not issue tokens or alter guards.

## Mail links and password-backed sessions {#mail-and-sessions}

The preset uses Fortify's `POST /forgot-password`, `POST /reset-password`, `PUT /user/profile-information` and `PUT /user/password` endpoints. Send JSON with `Accept: application/json`; keep the current CSRF cookie for Cookie authentication. Successful updates return 200; invalid fields or reset credentials return 422. Verification links require the authenticated owner, the current email hash, a valid signature and an unexpired timestamp. Successful JSON verification returns an empty 204; repeated verification is idempotent. Changing the email clears verification and sends a link for the new address.

The API preset also has minimal browser login, reset and verification pages. When a logged-out browser opens a verification link, login preserves the original signed URL. The receipt reads the stored verification state: adding `?verified=1` cannot verify an account. Configure the mail transport before relying on delivery. Setting `USER_EMAIL_VERIFICATION=false` disables verification routes and these verification emails while leaving password recovery available.

For the installed starter, login and valid remember-cookie recovery immediately record the existing password fingerprint. The Guard rejects a missing or outdated fingerprint on subsequent authentication reads. Password changes preserve the current browser session and reject other password-backed sessions; reset also rotates the remember token. Standalone authentication keeps its existing defaults.


### Verification identifiers and model keys

Signed verification URLs use the model primary key, which can differ from the authentication identifier. The default User implements `get_email_verification_identifier` on `MustVerifyEmail`; non-Eloquent implementations retain the authentication-identifier default. If your model authenticates by email or another field, add this method to its existing `MustVerifyEmail` implementation, returning the actual Eloquent key:

```rust
fn get_email_verification_identifier(
    &self,
) -> openrust::auth::AuthResult<openrust::auth::AuthIdentifier> {
    self.get_key()
        .ok_or(openrust::auth::AuthError::InvalidIdentity)?
        .try_into()
}
```

`VerifyEmail::send` and `VerifyEmail::url` use this identifier. `EmailVerificationRequest` checks the authenticated concrete identity, route key and current email hash; another model with the same authentication identifier is not interchangeable. The existing `auth::email_verification::set_verification` helper updates by the actual model primary key on its own connection, with an exact current-email and verification-state condition. `EmailVerificationRequest` dispatches `Verified` only for the successful transition; the helper itself does not dispatch save events. Concurrent valid requests remain idempotent. The application Date clock also supplies the persisted verification timestamp. No change to the signature algorithm is required.

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


### Other guards and polymorphic token owners

The suite scopes its credential version and mandatory login fingerprint to the selected session Guard, matching Eloquent provider configuration and actual connection/table. An independent admin table does not need the suite's credential column. Native login, password rehash and remember Cookies for other identities retain their behavior. Revoking a suite user's devices does not revoke an unrelated owner with the same numeric ID.

Sanctum can authenticate other registered token models. They remain usable on their own routes, but suite account-management routes reject another user-model type with 401. The suite adds its owner check instead of replacing your existing validator. Use `Sanctum::add_token_owner_validator` to append a check; every check must succeed, and errors fail authentication. `validate_token_owner_using` keeps its explicit replacement behavior.

Manual framework users may still use the existing global `auth.session_credential_version_attribute` setting. Without `auth.session_credential_version_scope`, it retains the original global behavior. The optional scope contains `guards` and `tables` (each table has `connection` and `table`); malformed scopes fail rather than silently enabling an unrestricted fallback. The installed suite assembles this scope from its configuration. If you wire runtime providers or differently named model aliases manually, explicitly verify the selected guards/connections and set the scope accordingly before serving requests; a default User fixture does not prove those custom combinations.

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
      "email_verified_at": null,
      "avatar_url": null
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

This section covers device login and TOTP/recovery challenges. See [stateless registration and recovery](#client-mail), [recent confirmation and personal tokens](#account-tokens), and [browser sessions](#browser-sessions) for the corresponding account operations.

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

For first-party Cookie/SPA clients, retain the normal CSRF flow and send credentials. `POST /user/confirm-password` records confirmation time and the suite identity snapshot in the current Session. Then `POST /api/user/tokens` with `device_name` and `abilities`. A separate opaque `proof` is not required for this browser mode. `POST /api/auth/confirm` also accepts a stateful Cookie request and returns201 without `data`. Another browser session does not inherit confirmation; missing/expired confirmation returns423 with localized code `password_confirmation_required`. A missing CSRF `token` remains419.

PAT proofs are hashed in the application's database, bound to the `user`, `current` PAT, purpose, credential version and effective factor/email state. `auth.password_timeout` is a positive number of seconds, default10800. Proofs expire without waiting for cleanup; wrong devices/purposes, tampering and replay return401. User and `token` row locks cover issuance/revocation. A late expiry or a cancelled model event rolls back the database mutation. Password callbacks may support an application-specific identity flow; an account without a local password is never automatically confirmed. Stateless Passkey reauthentication remains a separate pending adapter.

Password errors return422; five failed confirmations within a minute cause429 with `Retry-After`. Missing `token` capabilities return403. All displayed messages use the request locale. Proofs, passwords and `token` secrets are excluded from old input. Responses containing a new proof/token use `no-store`. See [browser sessions](#browser-sessions) for device management. See [account deletion](#account-deletion).

### Log out all devices

`POST /api/auth/logout-all` invalidates every previous suite session, remember Cookie and personal access token, including the caller's token. It does not change the password. A PAT requires `account:update` and a single-use confirmation for `purpose: "devices"`; Cookie authentication requires the native recent confirmation and CSRF. Success is 204 with no body. Use `/api/auth/logout` to revoke only the current PAT.

```sh
curl -i -X POST http://localhost:8000/api/auth/confirm \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: application/json' \
  -d '{"purpose":"devices","password":"long-password-123"}'

curl -i -X POST http://localhost:8000/api/auth/logout-all \
  -H 'Authorization: Bearer <token>' -H 'X-Auth-Confirmation: <proof>' \
  -H 'Content-Type: application/json' -d '{}'
```

The durable credential version changes under the same user lock as token issuance; proof expiry is rechecked after model events and before commit. A cancelled or failed save rolls back the revocation. A failed attempt may consume the proof, so confirm again before retrying. Successful revocation does not delete all session files or scan Redis keys: subsequent authentication reads reject the old credential version on every node. Previously issued token rows remain available for explicit management or pruning, but cannot authenticate. Requests already executing are not retroactively cancelled. Sign in again to establish new credentials.

This operation and [device enumeration](#browser-sessions) are separate capabilities; revocation does not claim that a device list is available for every session driver.

## Stateless registration and recovery {#client-mail}

The explicitly installed suite registers these routes beneath `api.prefix` (default `/api`). They reuse Fortify Actions, validation, the password broker and notifications; standalone Fortify routes retain their Cookie/CSRF behavior.

| Method | Path | Success |
| --- | --- | --- |
| POST | `/api/auth/register` | 201, safe user resource; no login or token issuance. |
| POST | `/api/auth/forgot-password` | 200, localized reset-link status. |
| POST | `/api/auth/reset-password` | 200, localized reset status; the reset credential is single-use. |
| POST | `/api/auth/email/verification-notification` | Bearer or stateful Cookie authentication; 202 when sent, 204 when already verified. |

Registration, password recovery and verification routes follow their respective feature settings. Send credentials in a JSON body, never in the query string. API authentication routes reject query credentials with 400 and return `Cache-Control: no-store`, including authentication, validation and rate-limit failures. Stateful requests still require CSRF; a JSON response does not remove this requirement.

```sh
curl -i -X POST http://localhost:8000/api/auth/register \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  -d '{"name":"Developer","email":"developer@example.test","password":"long-password-123","password_confirmation":"long-password-123"}'

curl -i -X POST http://localhost:8000/api/auth/forgot-password \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  -d '{"email":"developer@example.test"}'
```

After registration, explicitly complete [token login](#client-tokens). To verify from a native client, open the **original signed URL from the email** with `Authorization: Bearer <token>` and `Accept: application/json`. Do not relocate the signed route under `/api`, append credentials to it or rebuild its query. Invalid signatures, expired links and another user's link fail. Verification returns 204; repeating a valid link is idempotent. This web landing route may issue an anonymous session Cookie, but Bearer verification does not log a browser in. A browser can instead use the existing mail-login landing page.

For password recovery, extract the reset credential and email from the original mail link and POST them with `password` and `password_confirmation` to `/api/auth/reset-password`. A missing, expired or consumed credential returns 422. Concurrent submissions have one winner. Reset invalidates existing suite sessions and personal access tokens; log in again with the new password. Messages follow the selected application language. The application Date clock governs verification signatures and password reset expiry.

Configure an actual mail transport before relying on email. Synchronous delivery failures return an error, not a sent acknowledgement; retry the verification notification explicitly after fixing delivery. This route does not promise queued retries or external inbox delivery. Application notification callbacks remain replaceable.

## Application user resource {#user-resource}

Both presets publish `app/http/resources/user_resource.rs`. Its `UserResource` implements the native `JsonResource` trait. Edit `to_array` to select public fields; the default delegates to the safe identity and avatar projection. Do not serialize the entire user model. The generated Provider selects this resource for the owning application:

```rust
openrust_fortify::starter::UserResource::use_resource::<
    crate::app::models::User,
    crate::app::http::resources::user_resource::UserResource,
>(app)?;
```

The same resource transforms JSON registration/login, two-factor completion, current-user responses and the user inside personal-token grants. It receives the real request and runs native `prepare_response` before `to_array`. It must return a JSON object; preparation/serialization errors propagate. This is nested resource data: resource `wrap`, `with`, status and HTTP response hooks do not change the surrounding authentication response. Replace the existing Fortify response contract to customize that entire response. HTML account and mail pages retain their internal safe fields, so renaming API fields does not break the pages.

Repeat installation preserves this file; `--force` replaces it. Existing standalone file modules are reused. Inline or incompatible conditional resource modules stop installation before user-suite writes with a manual wiring error. Each application keeps its own selection; there is no process-global callback.

## Profile and password API {#user-updates}

Both presets provide `PUT /api/user/profile-information` and `PUT /api/user/password`; the API prefix is configurable. They require the selected user model and `account:update`. Profile updates, including name-only changes, require recent confirmation for `email`; password changes use `password`. A PAT proof is bound to the actual token, user and purpose, expires, and is single-use. It is never a URL parameter.

```shell
# First obtain an email-purpose proof with the current password:
curl -X POST "$APP_URL/api/auth/confirm" \
  -H "Accept: application/json" -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"purpose":"email","password":"current-password"}'
# Copy data.proof into PROOF, then update the profile:
curl -X PUT "$APP_URL/api/user/profile-information" \
  -H "Accept: application/json" -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -d '{"name":"Alice","email":"alice@example.test"}'
```

To change a password, obtain a new proof with `purpose: "password"` and PUT `password` plus `password_confirmation` to `/api/user/password`. This API uses the proof instead of asking for `current_password` a second time. Success is 200 with the normal localized empty-payload response. Password changes invalidate old PATs, so token clients must sign in with the new password. Browser clients use native Session confirmation and CSRF instead of a PAT proof. Their current session is synchronized and rotated; other old sessions and remember cookies fail. Refresh the CSRF token after rotation and confirm again before the next sensitive operation. The configured Fortify session guard must authenticate that browser identity.

Changing an email clears verification and sends the native verification notification to the new address; old email links fail. Proof expiry during a save, cancellation and transaction errors roll back the database mutation. Wrong or reused PAT proofs return 401, missing ability 403, field errors 422, unconfirmed browser sessions 423, and invalid CSRF 419. A failed mutation may consume its proof; confirm again to retry. Notification delivery and password-broker cleanup happen after the user transaction, as in the native flow: an error there does not undo the already committed profile/password. Check the current state before retrying. SMTP delivery guarantees remain those of the configured mail driver.

Existing app Actions remain replaceable. The API calls `validate_for_transaction` before taking the user lock, then `update_in_transaction` using the supplied transaction; it does not call a legacy Action that could save through another pool connection. The default profile validator uses the actual model table/connection. A custom password Action must use `change_password_in_transaction` from `credentials` to save the hash, increment the persistent credential version and clear the remember token. All writes must use the supplied transaction and return the same user/model/connection. The suite commits only after rechecking the proof deadline. The installer appends these methods to stock Actions while preserving comments; custom legacy Actions stop with manual wiring instructions, and complete custom transaction methods are preserved. Standalone Fortify Actions keep their original entry points.

## Password reset identity binding {#password-token-binding}

The installed user Provider calls `bind_password_tokens` for its selected model. The suite binds reset records to that model's actual primary key, connection/table scope, reset email, password hash and optional credential version. An old token cannot reset a different account that later registers the same email, and a late write from an old credential snapshot cannot authorize a reset after a password/version change. This also distinguishes accounts with no local password hash. Password rehashing changes the snapshot and therefore invalidates old reset links. The shared user lock also rechecks the reset email before saving: a token claimed before a concurrent email change cannot mutate the account after that change commits.

The existing database/cache repositories, 64-character email token, expiry, throttle and atomic consumption remain in use; no migration or parallel token store is added. Database and configured cache drivers apply the same binding. The binding is scoped to the selected user model and named broker; standalone Fortify and other user types retain their original behavior. Existing brokers read the same application-local rule registry, and repeated suite boot wiring is idempotent.

Enabling this binding invalidates earlier unbound reset links: request a new email. All application/Worker nodes must use the same model and binding configuration. Custom wrappers returned by a model's reset capability, or additional runtime tenant scopes, require explicit matching binding rules. Advanced apps can add named rules through `Password::bind_user_tokens_using`; distinct names compose, the same name replaces its rule, and callback errors fail instead of issuing unbound tokens. Keep rule names and values stable across nodes, and never return passwords or the computed binding to clients. Use the existing forgot/reset endpoints for normal application code.

### Two-factor account management {#user-two-factor-management}

Set `USER_TWO_FACTOR=true` after installing the user suite and applying its migrations. The default remains disabled. The complete profile adds an account-page link to `/user/two-factor-authentication`; it uses native Fortify forms, CSRF, the configured Session guard and application translations. Enable, scan the authenticator QR code or enter its setup key, enter a TOTP code when confirmation is required, and save the eight recovery codes offline. The page also supports regenerating recovery codes and disabling the factor. Setup and recovery details use `no-store` and `no-referrer`, are never stored as flash input, and require recent password confirmation. Enrollment or confirmation changes invalidate an older confirmation snapshot: confirm your password again when prompted. Hiding setup details does not disable the factor.

The API-only profile exposes the same management operations under the configured API prefix without publishing these pages:

| Method | Path after `/api` | Result |
| --- | --- | --- |
| GET | `/user/two-factor-authentication` | Safe `enabled`, `has_secret`, `confirmation_required` state; `account:read` required. |
| POST | `/user/two-factor-authentication` | Enable; return setup SVG, URL, `secretKey` and `recovery_codes`. |
| POST | `/user/confirmed-two-factor-authentication` | Confirm using `code`; return safe factor state. |
| DELETE | `/user/two-factor-authentication` | Disable; empty success data. |
| GET | `/user/two-factor-qr-code` | `svg` and `url`. |
| GET | `/user/two-factor-secret-key` | `secretKey`. |
| GET | `/user/two-factor-recovery-codes` | `recovery_codes` array inside `data`. |
| POST | `/user/two-factor-recovery-codes` | Generate and return replacement `recovery_codes`. |

Except for the safe-state GET, every operation requires `account:update` and a fresh single-use `factors` proof from `POST /api/auth/confirm`. Send it as `X-Auth-Confirmation`, or as a JSON `proof` field for a body-bearing request. Proofs in URL query parameters return 400; they must not appear in access logs, history or copied links. Fetch a new proof for the next operation, including after an invalid code; proofs cannot be replayed or transferred to another token. Cookie clients use their own recently confirmed Session plus CSRF instead of a PAT proof. Factor management does not issue a new access token.

```bash
# Obtain a proof for this one operation with the current PAT.
curl -X POST http://localhost:8000/api/auth/confirm \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"purpose":"factors","password":"your-password"}'

# Copy data.proof to PROOF, then enable the authenticator.
curl -X POST http://localhost:8000/api/user/two-factor-authentication \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -H 'Content-Type: application/json' -d '{}'
```

All writes and sensitive reads use the common user lock and same-connection transaction, checking the current token and credential snapshot before returning. Invalid codes return 422, missing or consumed proofs return 401, insufficient token abilities return 403, and disabled feature routes return 404. Login continues to require TOTP or an unused recovery code once the factor is active. Standalone Fortify applications retain their existing route responses; the additional API envelope and stricter confirmation checks belong to this installed suite.

### Browser profile and password forms {#web-user-updates}

The account page provides profile and password forms with a confirmation link. Native `PUT /user/profile-information` and `PUT /user/password` share the application Actions, user write lock and same-connection transaction with the API above. Name changes also require recent confirmation. Unconfirmed browsers redirect to `password.confirm`, then return to the account page to resubmit; passwords are never retained in old input. JSON negotiation keeps the localized 423 response. For password accounts, the browser password form also validates `current_password` against the configured Fortify guard.

Cookie confirmation binds the user, email, credentials and two-factor state at confirmation time together with the Session timestamp. An old timestamp cannot authorize changes after another device changes the email or factors. Confirm again before continuing. A password change preserves the current browser and invalidates old sessions and PATs. Updates rotate the Session and CSRF token: JSON/SPA clients must refresh CSRF, while ordinary browsers follow the redirect to a new form. Standalone Fortify applications without the user suite retain native behavior.

Browser feedback retains the native flash status codes and translates their display through `lang/<locale>/user.json`, such as `password-updated`; override the corresponding entry to customize it. JSON success messages continue to use ApiResponse localization. Existing browser confirmations without the suite snapshot require confirmation again after upgrading.

## Authentication mail queues and development preview {#auth-mail-queue}

The email-verification switch applies to the concrete starter user type, not every verification notification in the application. Other models retain their own verification flow. Advanced providers can use `VerifyEmail::send_when::<User>(app, |user| Ok(true))` to decide whether that type sends; returning an error fails the send. This condition is evaluated before rendering or enqueueing, and remains isolated by application.

Mail is synchronous by default, matching the native notification flow. To send through a separate Worker, configure the generated `config/user.rs` after installation:

```rust
"mail": {
    "queue": get(env, "USER_MAIL_QUEUE", Value::Null)?,
    "connection": get(env, "USER_MAIL_QUEUE_CONNECTION", Value::Null)?,
},
```

```dotenv
QUEUE_CONNECTION=database
USER_MAIL_QUEUE=auth-mail
# Optional; omission uses queue.default.
USER_MAIL_QUEUE_CONNECTION=database
```

Run the existing migrations and start the consumer:

```sh
./artisan migrate
./artisan queue:work database --queue=auth-mail --tries=3 --backoff=10
```

Registration/resend verification and password recovery use the named queue. Disabling email verification creates no verification jobs; recovery remains available. An omitted or `null` queue restores synchronous delivery. Empty names, commas and surrounding whitespace are invalid. Synchronous, discarding and in-process drivers, including failover entries of these types or cycles, are rejected. Persistent connections must also be registered and configured for the selected driver. For Redis, replace both `database` connection names above with the configured Redis queue connection name. Worker `--queue` must match; retry, backoff and timeout retain native semantics.

The kit configures `ResetPassword` and `VerifyEmail` separately for the concrete user type; other applications and models retain their own policies. Queued link payloads are always encrypted and use native after-commit notification dispatch. Producers and Workers must share `APP_KEY`, the application URL, model and queue configuration. Workers resolve mail transports and credentials from their own configuration. Never log passwords, signed URLs or reset tokens. A mail connection name may be stored in the job without serializing its SMTP password.

For independent policies, call `ResetPassword::queue_for::<User>(app, Some(options))` or `VerifyEmail::queue_for::<User>(app, Some(options))` in a Provider, using `openrust::auth::notifications::AuthMailQueueOptions`; `None` restores synchronous delivery for that notification. Custom URL/mail callbacks remain active. Models exposing reset/verification through wrapper objects are rejected by automatic registration; register the actual capability type explicitly instead of silently falling back. Upgrades preserve an existing `config/user.rs`: merge the group above and redeploy the Provider and Workers.

A successful request means enqueued, not received. Native Workers release and retry temporary provider failures, then use the configured failed-job store when attempts are exhausted. SMTP may duplicate delivery if a Worker crashes after provider acceptance but before queue acknowledgement; exactly-once delivery is not promised. Reset tokens remain single-use and verification links idempotent.

Use an isolated [Mailpit](https://mailpit.axllent.org/docs/install/docker/) instance for preview without real recipients:

```sh
docker run --rm --name openrust-auth-mailpit \
  --tmpfs /data \
  -e MP_DATABASE=/data/mailpit.db \
  -p 127.0.0.1:1025:1025 -p 127.0.0.1:8025:8025 \
  axllent/mailpit:v1.34.0
```

```dotenv
MAIL_MAILER=smtp
MAIL_SCHEME=smtp
MAIL_HOST=127.0.0.1
MAIL_PORT=1025
MAIL_USERNAME=null
MAIL_PASSWORD=null
MAIL_FROM_ADDRESS=hello@example.test
```

Open `http://127.0.0.1:8025`. These addresses assume the app runs on the host; containerized apps use the mail service name on their shared network. Remove an existing `MAIL_URL` or set it to `null`, since it overrides individual settings. Preview data disappears when this instance stops. This is not a production mail service: real provider credentials, production TLS, delivery and final receipt require separate acceptance.

## Browser sessions {#browser-sessions}

The full preset also publishes `GET /user/sessions`. Its native HTML forms revoke one or other sessions, keep CSRF and recent-password confirmation, then redirect with a localized status message. The API-only preset does not publish this account page.

When upgrading a preserved `config/user.rs`, merge the following group manually; an older file does not read `USER_DEVICES` automatically. Installers keep application configuration intact.

```rust
"devices": {"enabled": get(env, "USER_DEVICES", true)?},
```

The installed kit enables `user.devices.enabled` by default. Use native `SESSION_DRIVER=database` and run the published `0001_01_01_000023_create_user_browser_sessions_table.rs` migration. The separate index follows the User connection and isolates the model, database, table, real primary key and Guard; it does not change the original users migration. Existing sessions without an index must sign in again. File, Redis and replacement database handlers need an explicit `USER_DEVICES=false`; this removes these routes rather than pretending to enumerate those stores. The credential-version based all-device logout remains available. The installer assigns the current timestamp when publishing this migration, following the normal package migration rules.

| Endpoint | Result |
| --- | --- |
| `GET /api/user/sessions` | 200; `data` contains the caller's browser session summaries. A PAT requires `account:read`. |
| `DELETE /api/user/sessions/{session}` | 204; revoke the opaque device ID. Unknown or another user's IDs return404. |
| `DELETE /api/user/sessions` | 204; revoke other browser sessions. Preserve the current Cookie session; a PAT has no current browser, so it revokes all browser sessions. |

Deletion requires `account:update` for a PAT and recent confirmation for purpose `devices`. A Cookie client confirms through `POST /user/confirm-password` and retains CSRF protection. A token client obtains a single-use proof through `POST /api/auth/confirm`, then sends it in `X-Auth-Confirmation`. Missing Cookie confirmation returns423. With valid authorization, repeating an already recorded revocation returns204. These operations leave independent PATs valid; use [all-device logout](#credential-lifecycle) to revoke every credential.

```sh
curl "$APP_URL/api/user/sessions" -H "Authorization: Bearer <token>" -H "Accept: application/json"
curl -X DELETE "$APP_URL/api/user/sessions/<opaque-device-id>"   -H "Authorization: Bearer <token>" -H "X-Auth-Confirmation: <proof>" -H "Accept: application/json"
```

A summary contains `id`, `ip_address`, `user_agent`, `last_active` (Unix seconds) and `is_current_device`. PAT responses mark every item as non-current. The ID is a server-generated lowercase hash, not a credential. Raw session IDs, encrypted payloads, CSRF values and login fingerprints are never returned. Activity is not an online-presence guarantee. Native session decoding and Guard fingerprints establish ownership; the single `sessions.user_id` metadata column cannot represent every Guard sharing one browser.

Revocation is persisted under the same user lock as sensitive account operations. A stale request writing its old session back, or regenerating its ID after revocation, cannot create a new accepted device. Previously claimed Cookie confirmation is rechecked inside the transaction before issuing a PAT. Already executing work is not retroactively cancelled. Native remember tokens are shared by a user: revoking a browser clears that user's old remember tokens for all browsers while preserving other valid active sessions. A policy rejection clears only the affected Guard, preserving another Guard and unrelated session data.

The kit attaches `TrackBrowserSessions<User>` to existing `web` and `api` groups without replacing their Cookie, Session or CSRF middleware. Custom session groups may add `openrust_fortify::starter::devices::TrackBrowserSessions::<User>::new()` after `StartSession`. Keep the same `APP_KEY`, previous keys, database and session configuration across application nodes; retain previous keys while old encrypted sessions remain valid.

```sh
./artisan auth:prune-browser-sessions --hours=24
```

This removes records whose session expiry is older than the selected retention, including revoked records; `--hours=0` removes already expired records immediately. It is idempotent and does not remove active records, users, PATs or external session data. Schedule it using the normal scheduler; expiry and revocation take effect before cleanup.

## API token management page {#token-management-page}

The full preset provides `GET /user/api-tokens`, linked from the account page. It uses the native web Guard, Session and CSRF middleware; a browser does not need to create a PAT to manage PATs. The API-only preset omits this page and keeps the [token APIs](#account-tokens).

Confirm your password through the page link, enter a token name and explicitly select allowed abilities. `POST /user/api-tokens` issues a token using the same locked credential transaction and configured 30-day policy as the API. Unchecked abilities are not silently granted; selecting none or a server-disallowed ability fails validation. The configured unverified-email restrictions also apply.

The resulting page displays the plain-text token once. Copy it immediately and store it securely. A later GET only shows safe metadata; the token is not placed in flash data, the URL or a Cookie. Responses use `Cache-Control: no-store` and the page uses `Referrer-Policy: no-referrer`. Do not put screenshots or page contents containing a fresh token in logs. The database stores only its native Sanctum hash.

Each token has a revoke form targeting `DELETE /user/api-tokens/{token}` through the standard method override. It requires the same recent confirmation and CSRF protection, verifies ownership under the user lock, then redirects with a localized status. Revocation does not log out the current browser. Applications may customize the published `resources/views/user/api-tokens.blade.html` without replacing the underlying authentication contract.

An empty server ability whitelist disables new token creation on the page while leaving existing-token revocation available.

## Reliable resource cleanup {#resource-cleanup}

The installed kit includes `openrust_fortify::starter::resource_cleanup::ResourceCleanup` for trusted application actions. This is the shared backend for profile-photo replacement and account deletion; see [profile photos](#profile-photos) for the photo endpoints; see [account deletion](#account-deletion) for its application action and endpoints.

The cleanup ledger and a native database queue job are written in the same user transaction. A rollback preserves the previous resources. Sync, Redis, a replaced database handler or a different database connection cannot provide this guarantee and are rejected for this queue. This does not change the application's default queue: other jobs may continue using Redis. Generated `config/user.rs` selects `USER_CLEANUP_CONNECTION=database` and `USER_CLEANUP_QUEUE=user-resources` by default; create the normal jobs table on the User database and use a dedicated same-database queue connection for custom User connections.

```sh
./artisan queue:work database --queue=user-resources --tries=3 --backoff=5
./artisan auth:prune-user-cleanups
# Explicit connection when the queue configuration has changed:
./artisan auth:prune-user-cleanups --database=mysql
```

Use the configured connection and queue names in the Worker command. Separate applications sharing a jobs table must use separate queue names; nodes of the same application must share their storage and database configuration. Workers read their own filesystem credentials, not credentials from a queue payload. Published application configuration is preserved on upgrades: merge the new `cleanup` group before overriding its environment variables.

An action calls `ResourceCleanup::enqueue` only after checking ownership and recent authentication, inside the supplied user transaction. Only independently named files in the kit's `profile-photos/` namespace are accepted; do not reuse their paths for another upload. The returned receipt is shown once, while only its hash is stored. `ResourceCleanup::status` returns limited `pending`, `failed` or `completed` state without personal information or file paths, and does not need a still-existing User. Receipts expire after seven days. A failed storage deletion remains trackable; native Worker retries or `queue:retry` can complete it. Duplicate delivery is safe because the operation and file deletion are idempotent. This is not a transaction across SQL and object storage.

The prune command only removes completed records after receipt expiry; it keeps pending and failed work, users, tokens and external files. It is idempotent and may be scheduled through the normal scheduler. Account deletion uses the same cleanup lifecycle.

## Profile photos {#profile-photos}

Both presets provide `POST /api/user/profile-photo` (multipart `photo`) and `DELETE /api/user/profile-photo`; the prefix follows `api.prefix`. They require the selected user and `account:update`. Obtain a fresh `avatar` proof from `/api/auth/confirm` for each PAT operation and send it in `X-Auth-Confirmation`. Cookie clients use recent password confirmation and CSRF. The full preset also adds upload/removal forms to `/home`; the API preset adds no account page.

`user.avatar` defaults to `enabled=true`, disk `public`, 2048 KiB and 4096 pixels per side. Set `USER_AVATAR=false` to remove the endpoints and forms. The default handler requires an explicitly public disk. Private avatars require an application-specific handler and resource. Validation checks actual image bytes and decoding, accepts JPEG/PNG/WebP only, and enforces the tighter of kit and Image limits. Client filenames are never used as storage paths.

```sh
# TOKEN is an account:update PAT; PROOF is a fresh avatar confirmation proof.
curl -X POST "$APP_URL/api/user/profile-photo" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -F 'photo=@avatar.png'
```

The default user resource adds nullable `avatar_url`; it never exposes storage paths or disk names. A first upload returns200. Replacing/removing a stored image returns202 with `meta.cleanup` containing `id`, a one-time `receipt`, `expires_at`, `status_url` and `status=pending`. A custom user resource controls its own avatar projection. Photo changes keep the browser session, CSRF and recent confirmation intact.

A delayed, durable reservation is committed before writing the new file. Attaching the image and completing that reservation share the user transaction. Failed or abandoned uploads are reclaimed after a 15-minute grace period; cancelled writes receive a final cleanup pass. An attached photo is protected from delayed/repeated delivery. The previous photo is queued for deletion in the same transaction as replacement. Run the [resource cleanup Worker](#resource-cleanup), including its retries, on every deployment.

```sh
# Do not put the receipt in a URL. No login cookie is needed for this limited lookup.
curl "$STATUS_URL" -H "X-Operation-Receipt: $RECEIPT"
```

Lookup returns limited pending/failed/completed state and file count. A missing/wrong/expired receipt returns404; a receipt query parameter returns400. Responses use `no-store` and `no-referrer`; Telescope masks the receipt header and `meta.cleanup.receipt`. Keep this receipt private. The public local disk is served through the native static-disk route when its URL has the application origin. Nodes and Workers must share the same disk root or object store; use `FILESYSTEM_PUBLIC_ROOT` for a shared local mount. CDN/object-storage URLs use their own serving configuration. Production storage ACL/CDN behavior needs deployment acceptance.

## Account deletion {#account-deletion}

Both presets provide `DELETE /api/user` with the configured API prefix. A PAT needs `account:update` and a fresh, single-use `delete` confirmation. Cookie clients use recent password confirmation and CSRF. The full preset adds a native deletion form to `/home`, submitting `DELETE /user`; it redirects to password confirmation when needed.

```sh
curl -X POST "$APP_URL/api/auth/confirm" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"purpose":"delete","password":"your-current-password"}'
# Read data.proof into PROOF, then submit it once:
curl -i -X DELETE "$APP_URL/api/user" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF"
```

Installation publishes `app/actions/fortify/delete_user.rs` and selects it in the application UserServiceProvider. Edit this `DeleteUser`, implementing `starter::deletion::DeletesUsers<User>`, to define business dependencies and deletion policy. Its optional `preflight` runs before credential revocation; `delete` must use the supplied user transaction. The default delegates to the native model deletion. Business rows and remote identity-provider accounts are not automatically deleted. A soft-delete policy requires the application's actual soft-delete model and schema; returning success without deleting is rejected.

To replace the action explicitly during provider boot:

```rust
openrust_fortify::starter::deletion::delete_users_using::<crate::app::models::User>(
    app,
    std::sync::Arc::new(crate::app::actions::fortify::delete_user::DeleteUser),
)?;
```

The user lock serializes deletion with token issuance and account changes. The same transaction revokes the credential version, physically removes all owned PATs and installed passkeys through their registered model events, removes outstanding kit challenges, revokes the browser-device index, queues the previous photo, and deletes the user. The selected credential stores must use the user connection; unsupported cross-connection cleanup fails. Model cancellation, dependency constraints or enqueue failure roll back database changes and leave the existing photo untouched. Reset links are bound to the original identity and become unusable; repository expiry/cleanup continues to manage old reset-token records.

After commit, all current-browser guards for that same user are logged out and the session/CSRF are rotated. An unrelated administrator identity and cart data remain. This does not cancel business requests already executing. Application model hooks must defer external side effects until commit.

Without external files, the API returns204 and the HTML form redirects to login. With a photo, both return202: JSON contains `meta.cleanup`; HTML shows a private status page with a refresh button. The account is already removed, while file cleanup is pending. Keep the receipt privately; use [the existing public receipt lookup and Worker](#resource-cleanup). The lookup needs no deleted user session. A storage failure stays failed/retryable, never becomes an implicit rollback of the committed account deletion. Do not put the receipt in URLs or logs.
