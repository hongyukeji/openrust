# Socialite

Socialite connects third-party login to the application’s HTTP client and Session. Its provider API follows Socialite 5.33.0 and includes GitHub, GitLab, Bitbucket, LinkedIn (legacy and OpenID), Slack (identity and OpenID), X/Twitter OAuth2, Google and Facebook. Twitter OAuth1 and a local-account login recipe are also available. For full OIDC RP and SSO, use the separate [OIDC package](oidc.md).

## Installation {#installation}

Enable the optional `socialite` Cargo feature when building or running the application. Package discovery registers `SocialiteServiceProvider` and the application-local `SocialiteManager`. It creates no routes or local user table automatically.

```sh
cargo run --features socialite -- artisan serve
```

## Configuration {#configuration}

Configure `services.github.client_id`, `client_secret` and `redirect` through `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `GITHUB_REDIRECT_URI`. A redirect beginning with `/` is appended to a valid absolute HTTP(S) `app.url`; an absolute callback is retained. `services.github.scopes` adds scopes to the driver defaults. Missing or invalid configuration fails explicitly. Use a registered GitHub OAuth application and its callback for provider login.

The same client ID, client secret and redirect settings apply to these additional drivers. The environment prefix is followed by `_CLIENT_ID`, `_CLIENT_SECRET` and `_REDIRECT_URI`:

| Driver / services key | Environment prefix | Default scopes |
| --- | --- | --- |
| `gitlab` | `GITLAB` | `read_user` |
| `bitbucket` | `BITBUCKET` | `email` |
| `linkedin` | `LINKEDIN` | `r_liteprofile r_emailaddress` |
| `linkedin-openid` | `LINKEDIN_OPENID` | `openid profile email` |
| `slack` | `SLACK` | `identity.basic,identity.email,identity.team,identity.avatar` |
| `slack-openid` | `SLACK_OPENID` | `openid email profile` |

`services.gitlab.host` / `GITLAB_HOST` defaults to `https://gitlab.com`. `GitlabProvider::setHost` retains a self-hosted path prefix and removes trailing slashes. The profile endpoint is `/api/v3/user`. Slack login credentials share the services object with the existing `slack.notifications` settings. Provider-specific callback registration and permissions are still required for real services.

`x` uses `X_CLIENT_ID`, `X_CLIENT_SECRET` and `X_REDIRECT_URI`. `twitter` uses the equivalent `TWITTER_` variables; set `TWITTER_OAUTH=2` to select OAuth2. The default value 1 selects Twitter OAuth1. Only the integer 2 selects OAuth2; string `"2"` retains the OAuth1 selection. The explicit `twitter-oauth-2` driver reads `services.twitter`, falling back to `services.twitter-oauth-2` only when the first object is null/missing. `x` similarly falls back from `services.x` to `services.x-oauth-2`; a present incomplete object fails instead of silently using fallback credentials.

`google` reads `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` and `GOOGLE_REDIRECT_URI`, with default scopes `openid profile email`. Ordinary access tokens call Google UserInfo with `prettyPrint=false` and Bearer authentication. Refresh keeps the supplied refresh token only when the response omits that field; an explicit null or rotated value is retained.

`facebook` uses `FACEBOOK_CLIENT_ID`, `FACEBOOK_CLIENT_SECRET` and `FACEBOOK_REDIRECT_URI`, with the default `email` scope and pinned Graph version `v23.0`. `provider.usingGraphVersion("v99.0")` changes this request’s authorization/token/profile paths. `fields(vec!["id".into(), "name".into()])` replaces the Graph fields; `asPopup()` and `reRequest()` override the corresponding optional authorization parameters. Graph requests send `access_token`, fields and an HMAC-SHA256 `appsecret_proof` when the client secret is nonempty and not `"0"`. Code exchange leaves Accept unspecified, and preserves `expires` while filling a missing/null `expires_in` without overwriting an existing value.

### WeChat and QQ {#wechat-and-qq}

WeChat login is grouped by application type under `services.wechat`; QQ uses `services.qq`. Browser driver names follow SocialiteProviders, while configuration and source modules share the WeChat group:

| Driver / entry | Configuration path | Login purpose | Environment prefix |
| --- | --- | --- | --- |
| `weixinweb` | `services.wechat.web` | WeChat website QR login | `WECHAT_WEB` |
| `weixin` | `services.wechat.official_account` | WeChat official-account web authorization | `WECHAT_OFFICIAL_ACCOUNT` |
| `miniApp()` | `services.wechat.mini_app` | WeChat Mini Program login | `WECHAT_MINI_APP` |
| `qq` | `services.qq` | QQ Connect website login | `QQ` |

For browser WeChat and QQ, append `_CLIENT_ID`, `_CLIENT_SECRET`, and `_REDIRECT_URI` to each prefix, mapped to `client_id`, `client_secret`, and `redirect`. Use each WeChat application's own AppID and AppSecret and register its corresponding callback domain. Mini Programs use the AppID/secret settings below and require no redirect. Keep credentials separate for all three WeChat application types.

Reuse the redirect and callback examples below, replacing `github` with the driver name. Website QR login uses `snsapi_login`; official-account authorization defaults to `snsapi_userinfo`. For silent authorization, call `provider.setScopes(vec!["snsapi_base".into()])` on `weixin`; this returns OpenID without fetching a nickname or avatar. Keep the default Session state validation for browser login and avoid `stateless()`.

WeChat code exchange retains OpenID in the current request. Before direct `userFromToken` calls, use `provider.setOpenId(openid)` with the OpenID belonging to that token and application. QQ resolves OpenID before fetching the profile; request UnionID with `provider.withUnionId(true)`, subject to provider permissions. Read UnionID from `user.attributes.get("unionid")` and use standard User methods for nickname and avatar. These drivers do not return email.

Bind local accounts using a trusted application/provider namespace and OpenID. Nicknames or UnionID alone do not authorize automatic account merging. Website and official-account OpenIDs must not be assumed identical. Applications own registration and binding flows. `refreshToken` uses each provider's refresh endpoint; rejected, expired, revoked, or incomplete identity responses return errors. Production setup requires real application credentials, an approved callback, and an authorization test with the provider.

When upgrading existing configuration, move `services.weixinweb`, `services.weixin`, and `services.weixin_mini_app` to `services.wechat.web`, `services.wechat.official_account`, and `services.wechat.mini_app`, respectively. Rename environment prefixes from `WEIXINWEB_`, `WEIXIN_`, and `WEIXIN_MINI_APP_` to `WECHAT_WEB_`, `WECHAT_OFFICIAL_ACCOUNT_`, and `WECHAT_MINI_APP_`. Old configuration locations are no longer read. Import custom provider types from `openrust_socialite::wechat::{WeixinProvider, WeixinWebProvider, MiniApp, Session}`.

### WeChat Mini Programs {#wechat-mini-program}

Mini Program login uses the EasyWeChat-style `miniApp().codeToSession(code)` entry point. Set `WECHAT_MINI_APP_APPID` and `WECHAT_MINI_APP_SECRET`, mapped to `services.wechat.mini_app.app_id` / `secret`, using the Mini Program's own AppID and AppSecret. No redirect is required. This entry is available with the `socialite` feature and through `Socialite::miniApp()`.

Call `wx.login()` in the Mini Program and send its `code` over HTTPS POST to your application's login endpoint. Exchange it on the server using the method below, bind the verified identity to a local account as described below, and issue your application's own Sanctum/Passport token or login session. Client-supplied OpenID, UnionID, nickname and avatar are not login credentials.

```rust
use openrust_socialite::{Result, SocialiteManager, two::User};

pub async fn mini_login(manager: &SocialiteManager, code: &str) -> Result<User> {
    let session = manager.miniApp()?.codeToSession(code).await?;
    Ok(session.user())
}
```

`Session.openid` identifies the user; `unionid` is optional and depends on WeChat returning it. The `session_key` is a server-only `Zeroizing<String>`. Session implements neither serialization nor Debug. `session.user()` creates a Socialite User containing only OpenID and optional UnionID; neither raw nor attributes contain the session key, and credentials is empty. Never return the session key to the Mini Program or use it as an application access token.

Each call exchanges its code independently, without caching identities or automatic retries. WeChat enforces code expiry and single use; obtain a fresh code with `wx.login()` after failure. There is no browser redirect, OAuth state, access token or refresh token. Nickname, avatar and phone authorization require separate handling. OpenIDs belong to the Mini Program AppID and are not automatically merged with official-account or website identities. Provider error codes, missing identity/key fields, invalid JSON and unsuccessful HTTP responses return errors; the default timeout is five seconds. Register and protect your application login endpoint, apply rate limits, and verify the integration on a real device with your own Mini Program credentials before deployment.

## Authentication {#authentication}

Register GET routes for the redirect and callback in your application’s web group. The callback returns the remote identity below; mapping it to an application user and logging that user into a local guard belong to the application. Identify a remote account by provider and provider ID.

```rust
use openrust::{
    contracts::container::Container as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};
use openrust_socialite::SocialiteManager;

pub fn redirect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let response = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .redirect().await.map_err(|_| HttpError::InvalidRequest)?;
        Ok(response.into())
    })
}

pub fn callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let user = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .user().await.map_err(|_| HttpError::InvalidRequest)?;
        Response::json(StatusCode::OK, serde_json::json!({
            "id": user.getId(), "name": user.getName(), "email": user.getEmail(),
        }))
    })
}
```

`driver("github", request)` and `with("github", request)` borrow the current request. The provider keeps its cached user within that borrow; it is never cached globally by the manager. `redirect` returns a native redirect response. The Session stores a random 40-character state, which the callback consumes before exchanging the code. Missing, mismatched, array-valued, empty or replayed state fails. `enablePKCE()` is optional; enable it on both redirect and callback providers to store a 96-character verifier and send its S256 challenge and verifier.

The manager and facade return `Provider`, whose common `redirect().await?` and `user().await?` work with either protocol. OAuth1 needs HTTP to obtain temporary credentials before redirecting. Select `manager.driver("github", request)?.oauth2()?` before using OAuth2 options such as `scopes`, `stateless` or `refreshToken`; select `.oauth1()?` for Twitter `scope` and `userFromTokenAndSecret`. A protocol mismatch returns an explicit error. The concrete OAuth2 provider retains its synchronous `redirect()`.

Twitter OAuth1 uses `services.twitter.scope` / `TWITTER_SCOPE` (optional) or `.scope("write")`, independently of OAuth2 `scopes`. The request-token query sends `x_auth_access_type` for a nonempty scope other than `"0"`. Temporary credentials are stored at Session `oauth.temp`; the callback requires both `oauth_token` and `oauth_verifier`, checks the temporary identifier before HTTP, and reads rather than consumes the stored credentials. The remote server owns single-use temporary-token enforcement. OAuth1 requires Session for this redirect/callback flow and has no `stateless()` mode.

## Optional Parameters {#optional-parameters}

Call `provider.with(serde_json::json!({"prompt":"select_account","login_hint":"alice@example.test"}).as_object().unwrap().clone())` before redirecting or exchanging a code. The values are merged after the ordinary fields, as in the fixed upstream implementation. Scalar strings, booleans and numbers are supported. `redirectUrl` changes this provider instance’s callback.

For Slack identity, the default authorization field is `user_scope`, with an empty `scope`. Call `provider.asBotUser()` on the redirect and callback providers to select `scope` and the token response’s top-level credentials; ordinary user credentials come from `authed_user`. The default user-scope fields are applied after `with` parameters, matching upstream. `BitbucketProvider` also supports the explicit `provider.getAccessToken(code).await?` path, which adds HTTP Basic client authentication; ordinary `user()` follows the inherited token response path.

## Access Scopes {#access-scopes}

The GitHub default is `user:email`. `scopes(["read:user".into()])` merges unique scopes, while `setScopes(vec!["read:user".into()])` replaces them. `getScopes()` returns the current list. GitHub receives comma-separated scopes. When `user:email` is selected, Socialite queries `/user/emails` and selects a primary, verified email; failure or no match produces `None`. Removing that scope retains the profile’s email.

GitLab, Bitbucket and both LinkedIn/OpenID drivers use space-separated scopes; Slack identity uses commas and Slack OpenID uses spaces. Bitbucket selects a primary, confirmed email whose type is `email`; email endpoint failure yields `None`. Legacy LinkedIn loads the localized profile and email separately, requests `vanityName` with `r_liteprofile`, and propagates email request failure.

## Stateless Authentication {#stateless-authentication}

Call `provider.stateless()` before `user()` when the caller explicitly manages authentication state outside Session. This disables the state check; it does not turn a token into a local user or create a Session. PKCE still needs Session when enabled, matching the separate upstream controls.

X/Twitter OAuth2 enables PKCE by default on both redirect and callback, so it needs Session even with `stateless()`. Its default scopes are `users.read users.email tweet.read`; authorization queries use RFC3986 encoding. In stateless mode the authorization URL carries the literal `state=state`, after optional parameters, while callback state checking remains disabled. Code exchange uses HTTP Basic and the form client secret; refresh uses Basic and a form with `grant_type`, `refresh_token` and `client_id`, without a form client secret. Profiles map `data.confirmed_email`, not an unverified email field.

## Retrieving User Details {#retrieving-user-details}

The user exposes `getId`, `getNickname`, `getName`, `getEmail`, `getAvatar`, `getRaw` and mapped `attributes` such as GitHub `nodeId`. The provider ID retains its JSON number/string type. `userFromToken(token)` retrieves a profile using an existing token. `refreshToken(token)` exchanges a refresh token. Credentials are explicit in `user.credentials`: `token`, `refreshToken`, `expiresIn` and `approvedScopes`; token strings use `Zeroizing<String>` and are not automatically serialized or printed by Debug. A remote access token is not a Passport or Sanctum credential.

Legacy LinkedIn adds `first_name`, `last_name` and `avatar_original`, using the preferred locale and exact 100/800-pixel image selection from upstream. LinkedIn OpenID adds given/family names, `email_verified` and the original picture. Slack variants expose `organization_id`. The `linkedin-openid` and `slack-openid` drivers use the access token at fixed UserInfo endpoints, as in the pinned package; they do not validate ID Tokens, perform Discovery or implement a full OIDC RP. Use the [OIDC package](oidc.md) for those features.

Google treats a token longer than 100 bytes with exactly two dots as a JWT, matching the pinned provider. It fetches the configured Google JWKS endpoint, verifies the key algorithm and signature through `jsonwebtoken` 11.1.0 with AWS-LC, and requires the exact scalar issuer `https://accounts.google.com` and audience equal to the client ID. It checks present `exp`/`nbf` and the `iat` fallback, preserving the pinned PHP NumericDate behavior. Invalid JWTs never fall back to Bearer UserInfo. This Socialite path accepts an absent `exp`, numeric date strings and null dates as upstream does; it does not perform Discovery, require a nonce or implement full OIDC RP validation. Use the [OIDC package](oidc.md) for full RP validation. Google raw data also adds `id`, `verified_email`, `link` and mapped `avatar_original`.

Facebook Limited Login detects a non-null `kid` in the untrusted token header, then requires RS256 with the selected Facebook JWKS key, exact `https://www.facebook.com` issuer, scalar client audience and a matching nonce. Use `provider.withNonce(expected)` before `userFromToken(token).await?` or the callback’s `user()`; otherwise the scalar `with` parameter `nonce` supplies the expected value. Explicit `withNonce` takes precedence. The nonce must come from the application’s stored authorization attempt. `withNonce` only sets the expectation; it does not generate, store or add an authorization nonce automatically. Failed signature/claims/nonce verification never falls back to Graph. Request options and `lastToken()` stay in the current provider; the retained token is zeroized. This matches the pinned Socialite Limited Login path, including its optional dates, and does not replace the full [OIDC RP protocol](oidc.md).

### Logging in a bound local account

Use the stable pair `(provider, provider_user_id)` to resolve an existing local account. The namespace is a trusted application setting identifying the provider/issuer; use different namespaces for different installations or tenants. Keep the remote ID opaque: `007`, `7`, `ABC` and `abc` can identify different accounts. A verified email alone does not authorize account linking. This recipe rejects an unknown binding; applications handle registration or binding through a separate authenticated and verified workflow.

Create a table with a binary unique pair and a foreign key. The `user_id` type must match your local `users.id`; this example uses the default unsigned BIGINT key:

```sql
CREATE TABLE social_identities (
    provider VARBINARY(128) NOT NULL,
    provider_user_id VARBINARY(255) NOT NULL,
    user_id BIGINT UNSIGNED NOT NULL,
    UNIQUE (provider, provider_user_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
```

Place this action in your application's action directory and call `login_bound_user(request, "github", &user).await?` after the callback's `user().await?`. Return 403 when it returns false, otherwise redirect to a fixed local account page. Register your existing user model with `AuthManager` and configure `auth.guards.web` with its Eloquent provider. The action uses the native Session Guard, rotates the session identifier during login and replaces the CSRF token. The binding lookup does not create accounts, update emails or save remote access tokens.

```rust
use openrust::{
    auth::{Auth, AuthIdentifier},
    contracts::{auth::StatefulGuard as _, session::Session as _},
    database::DB,
    http::{HttpError, Request},
};
use openrust_socialite::two::User;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Application recipe: existing bindings only; namespace comes from trusted application configuration.
pub async fn login_bound_user(
    request: &mut Request,
    namespace: &str,
    remote: &User,
) -> Result<bool, HttpError> {
    let subject = match remote.getId() {
        serde_json::Value::String(id) if !id.is_empty() => id.clone(),
        serde_json::Value::Number(id) if id.is_i64() || id.is_u64() => id.to_string(),
        _ => return Ok(false),
    };
    if subject.len() > 255 {
        return Ok(false);
    }
    let Some(binding) = DB::table("social_identities")
        .map_err(internal)?
        .whereExactText("provider", namespace)
        .whereExactText("provider_user_id", &subject)
        .first()
        .await
        .map_err(internal)?
    else {
        return Ok(false);
    };
    let id = AuthIdentifier::try_from(binding.get("user_id").ok_or(HttpError::InvalidRequest)?)
        .map_err(internal)?;
    {
        let mut guard = Auth::sessionGuard(request, Some("web")).map_err(internal)?;
        if guard
            .loginUsingId(&id, false)
            .await
            .map_err(internal)?
            .is_none()
        {
            return Ok(false);
        }
    }
    request.session_mut()?.regenerateToken()?;
    Ok(true)
}
```

For multiple providers, store the selected namespace at redirect and consume/check it before calling the callback provider. Register fixed routes for each provider and keep the callback namespace under application control. Preserve the normal state check; never accept a callback-supplied local user ID. Handle local logout through a CSRF-protected POST route using `Auth::sessionGuard(request, Some("web"))?.logoutAndInvalidate().await?`. Local logout ends this application's Session; provider token revocation and federated logout have separate provider contracts.


## Testing {#testing}

Use a controlled HTTP provider with `GithubProvider::withEndpoints` and application-local `manager.extend("github", Arc::new(provider))`. It customizes authorization/token/profile/email URLs while retaining the driver behavior. `ProviderDefinition` supports compiled protocol customization. `facades::Socialite::driver` / `contracts::Factory`, `Provider` and `User` expose the public contracts.

Socialite requests use the shared client’s explicit `redactPayload()` mode: observers and recordings retain status and route but omit query, authorization, request body, response body and response headers. The actual network request and returned response remain complete. Provider status and malformed-response errors do not print response credentials.

`ProviderEndpoints::new(authorize, token, user).email(email)` supplies explicit endpoint overrides to the new drivers’ `withEndpoints` constructors.

For provider authorization, register an application and callback with the provider and configure its credentials. Use a dedicated account when testing the login flow.

`GoogleProvider::withEndpoints(endpoints, jwks)` configures the controlled UserInfo and JWKS services.

`FacebookProvider::withEndpoints(endpoints, jwks)` overrides the three Graph/authorization endpoints and the JWKS service for controlled tests; paths containing `/v23.0/` follow `usingGraphVersion`.

Twitter OAuth1 uses signed HMAC-SHA1 requests and explicit `user.credentials.tokenSecret` alongside the access token. `userFromTokenAndSecret(token, secret).await?` calls the profile endpoint directly. Callback mapping includes URL fields, location, description and `avatar_original`; direct-credential mapping retains only League extra fields, matching upstream. Successful profiles are cached per credential pair in one request; changing either credential fetches again. `one::TwitterProvider::withEndpoints` and `manager.extendOAuth1` support controlled endpoints.
