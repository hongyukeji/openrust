# OpenID Connect (Extension)

## Installation {#installation}

The optional `openrust-oidc` package implements an OpenID Connect relying party using the authorization code flow with PKCE S256, Discovery, JWKS and UserInfo. Enable the root `oidc` Cargo feature and rebuild; package discovery registers `OidcManager` and the `Oidc` facade. Applications register their own fixed web login and callback routes. No routes are enabled by default. Passport remains an OAuth2 authorization server; it does not become an OIDC provider.

```sh
cargo +1.95.0 build --locked --features oidc --bin main
```

Enabling the package starts with an empty `oidc.providers` map. To select the `work` example explicitly, run `./artisan vendor:publish --tag=oidc-config`, add `mod oidc;` and `("oidc".to_owned(), oidc::load(env)?),` to the declarations and configuration map in `config/mod.rs`, then rebuild. Alternatively, explicit `init --extensions=oidc` publishes and registers this template. Publishing preserves existing files unless `--force` is supplied.

## Provider configuration {#configuration}

The published `config/oidc.rs` template contains `oidc.providers.work`. Set `OIDC_ISSUER`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` and `OIDC_REDIRECT_URI`; `OIDC_REPLAY_STORE` selects the replay cache. Omitting a secret uses public-client authentication; otherwise the default is `client_secret_basic`. Set `client_auth` explicitly to `client_secret_post` or `none` when required. Registered callback URLs must match exactly. Configuration example:

```json
{
"providers": {
    "work": {
        "issuer": "https://identity.example.com",
        "client_id": "registered-client",
        "client_secret": "read-from-environment",
        "redirect": "https://app.example.com/oidc/callback",
        "scopes": ["openid", "profile", "email"],
        "algorithms": ["RS256"],
        "replay_store": "database"
    }
}
}
```

## Authorization and callback {#authorization}

Use the same configured provider name in the login and fixed callback route. `redirect()` and `user()` are asynchronous. A provider owns a mutable request borrow; create a new provider for each callback. `prompt("login")` requests reauthentication and `max_age(60)` requires a sufficiently recent `auth_time` claim. Starting a new attempt for the same provider replaces the previous attempt in that Session.

```rust,ignore
use openrust_oidc::facades::Oidc;

// Inside an async handler; convert errors to your application's HTTP errors.
let response = Oidc::provider("work", request)?.redirect().await?;

// Inside the fixed callback handler.
let authentication = Oidc::provider("work", request)?.user().await?;
let identity = &authentication.identity;
```

## Verified identities {#identity}

`Authentication.identity` is a `VerifiedIdentity` constructed only after signature and claims validation. `issuer()` and `subject()` are the stable identity pair; `email()`, `email_verified()`, `claims()` and `user_info()` expose validated data. UserInfo must have the exact verified subject. A changed email does not inherit the ID Token’s verification flag: the UserInfo response must itself contain boolean `email_verified: true`. `Authentication.tokens` explicitly exposes zeroizing access/ID/optional refresh tokens and optional expiry; it has no Debug or Serialize implementation. Refresh-token grants are outside this RP client API.

## Local account binding {#local-binding}

Resolve an existing local account using the binary pair `(issuer, subject)`. Never merge accounts by email or accept a callback-supplied local user ID. The sample uses default unsigned BIGINT user keys. Unknown bindings return false; registration and linking require a separate authenticated workflow. Place the following action in your application, register the existing Eloquent user model with `AuthManager`, and call `login_bound_user(request, &authentication.identity).await?` after `user()`. Reject false with 403. Login uses the native Session Guard and rotates the Session identifier and CSRF token.

```sql
CREATE TABLE oidc_identities (
    issuer VARBINARY(2048) NOT NULL,
    subject VARBINARY(255) NOT NULL,
    user_id BIGINT UNSIGNED NOT NULL,
    UNIQUE (issuer, subject),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
```

```rust
use openrust::{
    auth::{Auth, AuthIdentifier},
    contracts::{auth::StatefulGuard as _, session::Session as _},
    database::Db,
    http::{HttpError, Request},
};
use openrust_oidc::VerifiedIdentity;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Application recipe: existing bindings only; issuer and subject come only from the verified identity.
pub async fn login_bound_user(
    request: &mut Request,
    remote: &VerifiedIdentity,
) -> Result<bool, HttpError> {
    let Some(binding) = Db::table("oidc_identities")
        .map_err(internal)?
        .where_exact_text("issuer", remote.issuer())
        .where_exact_text("subject", remote.subject())
        .first()
        .await
        .map_err(internal)?
    else {
        return Ok(false);
    };
    let id = AuthIdentifier::try_from(binding.get("user_id").ok_or(HttpError::InvalidRequest)?)
        .map_err(internal)?;
    {
        let mut guard = Auth::session_guard(request, Some("web")).map_err(internal)?;
        if guard
            .login_using_id(&id, false)
            .await
            .map_err(internal)?
            .is_none()
        {
            return Ok(false);
        }
    }
    request.session_mut()?.regenerate_token()?;
    Ok(true)
}
```

## Validation boundaries {#validation}

The RP requires HTTPS, exact issuer, client audience, nonempty ASCII subject up to 255 bytes, nonce, `exp` and `iat`. Multiple audiences require matching `azp`; a present `azp` must always match. It checks `nbf`, optional `at_hash`/`c_hash` and requested `auth_time`. It requires a unique nonempty `kid`, checks JWK usage/algorithm and accepts configured asymmetric algorithms only. The default is RS256; explicit options are RS384/512, PS256/384/512 and ES256/384. HMAC, unsigned tokens, token-supplied keys and remote key URLs are rejected.

Discovery endpoints default to the issuer origin. Add required HTTPS origins to `allowed_origins` explicitly (at most 16 additional origins). URLs with user credentials/fragments and authorization endpoints with reserved query parameters are rejected. Redirect following is disabled; HTTP responses are capped at 1 MiB. `allow_loopback_http: true` permits literal IPv4/IPv6 loopback HTTP only for local testing, never hostname-based HTTP. Defaults: attempt TTL 600 seconds (1–3600), per-request timeout 10 seconds (1–60), maximum ID Token age 600 seconds (0–86400), clock leeway 0 seconds (0–60), UserInfo enabled. Use a shared atomic database/Redis cache for multiple processes or replicas. The array cache only protects one process. Attempts are consumed even after a failed callback; begin a new login to retry. Local Session logout does not revoke OP tokens or terminate the OP Session.

## Browser SSO and logout {#sso-logout}

Configure each RP with its own client ID, application key, database and local user bindings. Sign in at the OP once; the second RP can authorize through the existing OP browser Session. Each RP still owns its local Session and resolves `(issuer, subject)` separately. `prompt("login")` asks the OP to authenticate again; `max_age` additionally checks the returned `auth_time`.

RP-initiated logout follows OpenID Connect RP-Initiated Logout 1.0 (2022-09-12). Set `OIDC_POST_LOGOUT_REDIRECT_URI` (`post_logout_redirect`) to a fixed callback registered at the OP. After `user()` and successful local account binding, call `Oidc::provider("work", request)?.remember_logout(&authentication)?`. This explicitly saves only the verified ID Token in the server-side Session. `LogoutHint` is opaque; changing the public token fields cannot replace this verified hint. Treat all Session storage and outbound logout URLs as sensitive.

Place this action in the application and register it on a CSRF-protected POST web route. Pass `false` for local logout and `true` to attempt OP logout:

```rust
use openrust::{
    auth::Auth,
    http::{HttpError, Request, Response, StatusCode},
};
use openrust_oidc::facades::Oidc;
use serde_json::json;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Register on a POST route in the native web group with CSRF protection.
pub async fn logout(request: &mut Request, federated: bool) -> Result<Response, HttpError> {
    let hint = Oidc::provider("work", request)
        .map_err(internal)?
        .take_logout_hint()
        .ok();
    Auth::session_guard(request, Some("web"))
        .map_err(internal)?
        .logout_and_invalidate()
        .await
        .map_err(internal)?;
    if federated
        && let Some(hint) = hint
        && let Ok(url) = Oidc::provider("work", request)
            .map_err(internal)?
            .logout_url(&hint)
            .await
    {
        return Ok(
            openrust::support::facades::Redirect::away(&url, Default::default())
                .map_err(internal)?
                .into(),
        );
    }
    Response::json(StatusCode::OK, json!({"local":true,"federated":false}))
}
```

`take_logout_hint()` consumes the hint before native `logout_and_invalidate()` clears the local Session and renews CSRF. `logout_url(&hint)` discovers and checks the allowed `end_session_endpoint`, adds the ID Token hint, client ID, fixed return URI and one-time state, then saves the state in the new anonymous Session. Missing support, unavailable Discovery or invalid metadata still leave local logout complete in this action. Do not take the hint or return destination from request parameters.

In the fixed return handler, call `Oidc::provider("work", request)?.complete_logout().await?` and return a local signed-out page. Completion checks Session binding, configuration, destination, expiry and a shared atomic replay claim. A failed return consumes its attempt; it never logs anyone in. It acknowledges the return state and does not prove that every remote credential was revoked.

Ending the OP browser Session leaves the other RP's existing local Session valid. Existing access and refresh tokens follow the OP's separate revocation policy. Front-channel/back-channel logout notifications and universal single logout are outside this implementation.


## Testing and deployment {#testing}

Register the application and callback with your identity provider and configure its issuer and client credentials. Use an account authorized for that application. For multiple RP replicas, share Session storage and an atomic database or Redis cache to prevent callback replay.
