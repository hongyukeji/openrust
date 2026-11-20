# 开发者文档边界整理：docs/en/packages/socialite.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/packages/socialite.md (迁移前)
+++ docs/en/packages/socialite.md (开发者正文)
@@ -1,6 +1,6 @@
 # Socialite

-Socialite connects third-party login to the application’s HTTP client and Session. The accepted scope now includes GitHub, GitLab, Bitbucket, LinkedIn (legacy and OpenID) Slack (identity and OpenID), X/Twitter OAuth2, Google and Facebook against fixed Socialite 5.33.0. Twitter OAuth1 and a local-account login recipe are also available. Full OIDC RP and SSO belong to API-O01/02.
+Socialite connects third-party login to the application’s HTTP client and Session. Its provider API follows Socialite 5.33.0 and includes GitHub, GitLab, Bitbucket, LinkedIn (legacy and OpenID), Slack (identity and OpenID), X/Twitter OAuth2, Google and Facebook. Twitter OAuth1 and a local-account login recipe are also available. For full OIDC RP and SSO, use the separate [OIDC package](oidc.md).

 ## Installation {#installation}

@@ -12,7 +12,7 @@

 ## Configuration {#configuration}

-Configure `services.github.client_id`, `client_secret` and `redirect` through `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `GITHUB_REDIRECT_URI`. A redirect beginning with `/` is appended to a valid absolute HTTP(S) `app.url`; an absolute callback is retained. `services.github.scopes` adds scopes to the driver defaults. Missing or invalid configuration fails explicitly. Use a registered GitHub OAuth application and its callback for real provider acceptance.
+Configure `services.github.client_id`, `client_secret` and `redirect` through `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `GITHUB_REDIRECT_URI`. A redirect beginning with `/` is appended to a valid absolute HTTP(S) `app.url`; an absolute callback is retained. `services.github.scopes` adds scopes to the driver defaults. Missing or invalid configuration fails explicitly. Use a registered GitHub OAuth application and its callback for provider login.


 The same client ID, client secret and redirect settings apply to these additional drivers. The environment prefix is followed by `_CLIENT_ID`, `_CLIENT_SECRET` and `_REDIRECT_URI`:
@@ -26,7 +26,7 @@
 | `slack` | `SLACK` | `identity.basic,identity.email,identity.team,identity.avatar` |
 | `slack-openid` | `SLACK_OPENID` | `openid email profile` |

-`services.gitlab.host` / `GITLAB_HOST` defaults to `https://gitlab.com`. `GitlabProvider::setHost` retains a self-hosted path prefix and removes trailing slashes. The fixed upstream driver uses `/api/v3/user`; this acceptance preserves that contract. Slack login credentials share the services object with the existing `slack.notifications` settings. Provider-specific callback registration and permissions are still required for real services.
+`services.gitlab.host` / `GITLAB_HOST` defaults to `https://gitlab.com`. `GitlabProvider::setHost` retains a self-hosted path prefix and removes trailing slashes. The profile endpoint is `/api/v3/user`. Slack login credentials share the services object with the existing `slack.notifications` settings. Provider-specific callback registration and permissions are still required for real services.

 `x` uses `X_CLIENT_ID`, `X_CLIENT_SECRET` and `X_REDIRECT_URI`. `twitter` uses the equivalent `TWITTER_` variables; set `TWITTER_OAUTH=2` to select OAuth2. The default value 1 selects Twitter OAuth1. Only the integer 2 selects OAuth2; string `"2"` retains the OAuth1 selection. The explicit `twitter-oauth-2` driver reads `services.twitter`, falling back to `services.twitter-oauth-2` only when the first object is null/missing. `x` similarly falls back from `services.x` to `services.x-oauth-2`; a present incomplete object fails instead of silently using fallback credentials.

@@ -101,11 +101,11 @@
 The user exposes `getId`, `getNickname`, `getName`, `getEmail`, `getAvatar`, `getRaw` and mapped `attributes` such as GitHub `nodeId`. The provider ID retains its JSON number/string type. `userFromToken(token)` retrieves a profile using an existing token. `refreshToken(token)` exchanges a refresh token. Credentials are explicit in `user.credentials`: `token`, `refreshToken`, `expiresIn` and `approvedScopes`; token strings use `Zeroizing<String>` and are not automatically serialized or printed by Debug. A remote access token is not a Passport or Sanctum credential.


-Legacy LinkedIn adds `first_name`, `last_name` and `avatar_original`, using the preferred locale and exact 100/800-pixel image selection from upstream. LinkedIn OpenID adds given/family names, `email_verified` and the original picture. Slack variants expose `organization_id`. The `linkedin-openid` and `slack-openid` drivers use the access token at fixed UserInfo endpoints, as in the pinned package; they do not validate ID Tokens, perform Discovery or implement a full OIDC RP. Those belong to API-O01.
-
-Google treats a token longer than 100 bytes with exactly two dots as a JWT, matching the pinned provider. It fetches the configured Google JWKS endpoint, verifies the key algorithm and signature through `jsonwebtoken` 11.1.0 with AWS-LC, and requires the exact scalar issuer `https://accounts.google.com` and audience equal to the client ID. It checks present `exp`/`nbf` and the `iat` fallback, preserving the pinned PHP NumericDate behavior. Invalid JWTs never fall back to Bearer UserInfo. This Socialite path accepts an absent `exp`, numeric date strings and null dates as upstream does; it does not perform Discovery, require a nonce or implement full OIDC RP validation. API-O01 has a separate protocol scope. Google raw data also adds `id`, `verified_email`, `link` and mapped `avatar_original`.
-
-Facebook Limited Login detects a non-null `kid` in the untrusted token header, then requires RS256 with the selected Facebook JWKS key, exact `https://www.facebook.com` issuer, scalar client audience and a matching nonce. Use `provider.withNonce(expected)` before `userFromToken(token).await?` or the callback’s `user()`; otherwise the scalar `with` parameter `nonce` supplies the expected value. Explicit `withNonce` takes precedence. The nonce must come from the application’s stored authorization attempt. `withNonce` only sets the expectation; it does not generate, store or add an authorization nonce automatically. Failed signature/claims/nonce verification never falls back to Graph. Request options and `lastToken()` stay in the current provider; the retained token is zeroized. This matches the pinned Socialite Limited Login path, including its optional dates, and does not replace the full API-O01 RP protocol.
+Legacy LinkedIn adds `first_name`, `last_name` and `avatar_original`, using the preferred locale and exact 100/800-pixel image selection from upstream. LinkedIn OpenID adds given/family names, `email_verified` and the original picture. Slack variants expose `organization_id`. The `linkedin-openid` and `slack-openid` drivers use the access token at fixed UserInfo endpoints, as in the pinned package; they do not validate ID Tokens, perform Discovery or implement a full OIDC RP. Use the [OIDC package](oidc.md) for those features.
+
+Google treats a token longer than 100 bytes with exactly two dots as a JWT, matching the pinned provider. It fetches the configured Google JWKS endpoint, verifies the key algorithm and signature through `jsonwebtoken` 11.1.0 with AWS-LC, and requires the exact scalar issuer `https://accounts.google.com` and audience equal to the client ID. It checks present `exp`/`nbf` and the `iat` fallback, preserving the pinned PHP NumericDate behavior. Invalid JWTs never fall back to Bearer UserInfo. This Socialite path accepts an absent `exp`, numeric date strings and null dates as upstream does; it does not perform Discovery, require a nonce or implement full OIDC RP validation. Use the [OIDC package](oidc.md) for full RP validation. Google raw data also adds `id`, `verified_email`, `link` and mapped `avatar_original`.
+
+Facebook Limited Login detects a non-null `kid` in the untrusted token header, then requires RS256 with the selected Facebook JWKS key, exact `https://www.facebook.com` issuer, scalar client audience and a matching nonce. Use `provider.withNonce(expected)` before `userFromToken(token).await?` or the callback’s `user()`; otherwise the scalar `with` parameter `nonce` supplies the expected value. Explicit `withNonce` takes precedence. The nonce must come from the application’s stored authorization attempt. `withNonce` only sets the expectation; it does not generate, store or add an authorization nonce automatically. Failed signature/claims/nonce verification never falls back to Graph. Request options and `lastToken()` stay in the current provider; the retained token is zeroized. This matches the pinned Socialite Limited Login path, including its optional dates, and does not replace the full [OIDC RP protocol](oidc.md).

 ### Logging in a bound local account

@@ -184,21 +184,21 @@

 For multiple providers, store the selected namespace at redirect and consume/check it before calling the callback provider. Register fixed routes for each provider and keep the callback namespace under application control. Preserve the normal state check; never accept a callback-supplied local user ID. Handle local logout through a CSRF-protected POST route using `Auth::sessionGuard(request, Some("web"))?.logoutAndInvalidate().await?`. Local logout ends this application's Session; provider token revocation and federated logout have separate provider contracts.

-The MySQL 8.4 acceptance application executes this action with real Eloquent users, database Sessions, HTTP token/profile endpoints and two isolated schemas. It verifies email collisions without account merging, provider/subject case and numeric distinctions, concurrent login, unique binding enforcement, session fixation rejection, Session restoration, CSRF/logout rotation, cross-app cookies, missing/deleted users, invalid/replayed state, revoked tokens and database failure. This is a controlled-provider application integration test; real provider credentials and callbacks are still required for account authorization.
+

 ## Testing {#testing}

-Use a controlled HTTP provider with `GithubProvider::withEndpoints` and application-local `manager.extend("github", Arc::new(provider))`. It customizes authorization/token/profile/email URLs while retaining the driver behavior. `ProviderDefinition` supports compiled protocol customization. `facades::Socialite::driver` / `contracts::Factory`, `Provider` and `User` expose the public contracts. Tests execute real HTTP plus two independent Session applications, PKCE, state consumption, scope/parameter mapping, verified email, refresh and failure paths; fixed PHP source checks protect the comparison fixture.
-
-Socialite requests use the shared client’s explicit `redactPayload()` mode: observers and recordings retain status and route but omit query, authorization, request body, response body and response headers. The actual network request and returned response remain complete. Provider status and malformed-response errors do not print response credentials. Controlled-provider acceptance does not claim real GitHub authorization; that still requires an OAuth application, credentials, callback domain and account.
-
-
-`ProviderEndpoints::new(authorize, token, user).email(email)` supplies explicit endpoint overrides to the new drivers’ `withEndpoints` constructors. Each driver runs against real controlled HTTP, including token form fields, authorization headers, profile query fields, optional email selection, Slack user/bot credentials, refresh and rejected/revoked credentials. Six driver mappings, scope fields and Slack special fields are compared with real pinned PHP methods. Real GitLab, Bitbucket, LinkedIn or Slack accounts have not been authorized by these controlled tests.
-
-X/Twitter tests exercise default PKCE over real HTTP and encrypted Session cookies, cross-application rejection, state replay, RFC3986 parameters, Basic exchange/refresh, malformed responses and remote revocation. Pinned PHP methods supply the mapping, fields and HTTP options oracle. Real X/Twitter application authorization still requires provider credentials, a registered callback and an account.
-
-`GoogleProvider::withEndpoints(endpoints, jwks)` configures the controlled UserInfo and JWKS services. Real HTTP tests replay 25 signed JWT cases against the original PHP Socialite/Firebase JWT methods, including wrong signatures, issuer/audience, missing or malformed claims, future/expired dates, algorithm confusion, rotation and cross-client isolation. Test PEM keys belong only to these fixtures. Real Google account authorization still requires a test application and callback.
-
-`FacebookProvider::withEndpoints(endpoints, jwks)` overrides the three Graph/authorization endpoints and the JWKS service for controlled tests; paths containing `/v23.0/` follow `usingGraphVersion`. Twenty-one original PHP Limited Login cases cover nonce precedence, issuer/audience, signature/expiry, missing keys and rotation; actual Graph HTTP checks HMAC proof, fields, expiry conversion, request isolation and redacted observations. Real Facebook authorization still requires an application, credentials, registered callback and account.
-
-Twitter OAuth1 uses signed HMAC-SHA1 requests and explicit `user.credentials.tokenSecret` alongside the access token. `userFromTokenAndSecret(token, secret).await?` calls the profile endpoint directly. Callback mapping includes URL fields, location, description and `avatar_original`; direct-credential mapping retains only League extra fields, matching upstream. Successful profiles are cached per credential pair in one request; changing either credential fetches again. `one::TwitterProvider::withEndpoints` and `manager.extendOAuth1` support controlled endpoints. Fixed League 1.12.0 / Socialite PHP comparisons and real HTTP/Session tests cover signatures, empty verifier, scope, credential cache, cross-app rejection, remote replay refusal and redacted failure paths. Real Twitter authorization still needs an application, credentials, registered callback and account.
+Use a controlled HTTP provider with `GithubProvider::withEndpoints` and application-local `manager.extend("github", Arc::new(provider))`. It customizes authorization/token/profile/email URLs while retaining the driver behavior. `ProviderDefinition` supports compiled protocol customization. `facades::Socialite::driver` / `contracts::Factory`, `Provider` and `User` expose the public contracts.
+
+Socialite requests use the shared client’s explicit `redactPayload()` mode: observers and recordings retain status and route but omit query, authorization, request body, response body and response headers. The actual network request and returned response remain complete. Provider status and malformed-response errors do not print response credentials.
+
+
+`ProviderEndpoints::new(authorize, token, user).email(email)` supplies explicit endpoint overrides to the new drivers’ `withEndpoints` constructors.
+
+For provider authorization, register an application and callback with the provider and configure its credentials. Use a dedicated account when testing the login flow.
+
+`GoogleProvider::withEndpoints(endpoints, jwks)` configures the controlled UserInfo and JWKS services.
+
+`FacebookProvider::withEndpoints(endpoints, jwks)` overrides the three Graph/authorization endpoints and the JWKS service for controlled tests; paths containing `/v23.0/` follow `usingGraphVersion`.
+
+Twitter OAuth1 uses signed HMAC-SHA1 requests and explicit `user.credentials.tokenSecret` alongside the access token. `userFromTokenAndSecret(token, secret).await?` calls the profile endpoint directly. Callback mapping includes URL fields, location, description and `avatar_original`; direct-credential mapping retains only League extra fields, matching upstream. Successful profiles are cached per credential pair in one request; changing either credential fetches again. `one::TwitterProvider::withEndpoints` and `manager.extendOAuth1` support controlled endpoints.
````

````diff
--- docs/en/packages/socialite.md (迁移前)
+++ docs/en/packages/socialite.md (开发者正文)
@@ -13,7 +13,6 @@
 ## Configuration {#configuration}

 Configure `services.github.client_id`, `client_secret` and `redirect` through `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `GITHUB_REDIRECT_URI`. A redirect beginning with `/` is appended to a valid absolute HTTP(S) `app.url`; an absolute callback is retained. `services.github.scopes` adds scopes to the driver defaults. Missing or invalid configuration fails explicitly. Use a registered GitHub OAuth application and its callback for provider login.
-

 The same client ID, client secret and redirect settings apply to these additional drivers. The environment prefix is followed by `_CLIENT_ID`, `_CLIENT_SECRET` and `_REDIRECT_URI`:

@@ -80,13 +79,11 @@

 Call `provider.with(serde_json::json!({"prompt":"select_account","login_hint":"alice@example.test"}).as_object().unwrap().clone())` before redirecting or exchanging a code. The values are merged after the ordinary fields, as in the fixed upstream implementation. Scalar strings, booleans and numbers are supported. `redirectUrl` changes this provider instance’s callback.

-
 For Slack identity, the default authorization field is `user_scope`, with an empty `scope`. Call `provider.asBotUser()` on the redirect and callback providers to select `scope` and the token response’s top-level credentials; ordinary user credentials come from `authed_user`. The default user-scope fields are applied after `with` parameters, matching upstream. `BitbucketProvider` also supports the explicit `provider.getAccessToken(code).await?` path, which adds HTTP Basic client authentication; ordinary `user()` follows the inherited token response path.

 ## Access Scopes {#access-scopes}

 The GitHub default is `user:email`. `scopes(["read:user".into()])` merges unique scopes, while `setScopes(vec!["read:user".into()])` replaces them. `getScopes()` returns the current list. GitHub receives comma-separated scopes. When `user:email` is selected, Socialite queries `/user/emails` and selects a primary, verified email; failure or no match produces `None`. Removing that scope retains the profile’s email.
-

 GitLab, Bitbucket and both LinkedIn/OpenID drivers use space-separated scopes; Slack identity uses commas and Slack OpenID uses spaces. Bitbucket selects a primary, confirmed email whose type is `email`; email endpoint failure yields `None`. Legacy LinkedIn loads the localized profile and email separately, requests `vanityName` with `r_liteprofile`, and propagates email request failure.

@@ -99,7 +96,6 @@
 ## Retrieving User Details {#retrieving-user-details}

 The user exposes `getId`, `getNickname`, `getName`, `getEmail`, `getAvatar`, `getRaw` and mapped `attributes` such as GitHub `nodeId`. The provider ID retains its JSON number/string type. `userFromToken(token)` retrieves a profile using an existing token. `refreshToken(token)` exchanges a refresh token. Credentials are explicit in `user.credentials`: `token`, `refreshToken`, `expiresIn` and `approvedScopes`; token strings use `Zeroizing<String>` and are not automatically serialized or printed by Debug. A remote access token is not a Passport or Sanctum credential.
-

 Legacy LinkedIn adds `first_name`, `last_name` and `avatar_original`, using the preferred locale and exact 100/800-pixel image selection from upstream. LinkedIn OpenID adds given/family names, `email_verified` and the original picture. Slack variants expose `organization_id`. The `linkedin-openid` and `slack-openid` drivers use the access token at fixed UserInfo endpoints, as in the pinned package; they do not validate ID Tokens, perform Discovery or implement a full OIDC RP. Use the [OIDC package](oidc.md) for those features.

@@ -185,13 +181,11 @@
 For multiple providers, store the selected namespace at redirect and consume/check it before calling the callback provider. Register fixed routes for each provider and keep the callback namespace under application control. Preserve the normal state check; never accept a callback-supplied local user ID. Handle local logout through a CSRF-protected POST route using `Auth::sessionGuard(request, Some("web"))?.logoutAndInvalidate().await?`. Local logout ends this application's Session; provider token revocation and federated logout have separate provider contracts.


-
 ## Testing {#testing}

 Use a controlled HTTP provider with `GithubProvider::withEndpoints` and application-local `manager.extend("github", Arc::new(provider))`. It customizes authorization/token/profile/email URLs while retaining the driver behavior. `ProviderDefinition` supports compiled protocol customization. `facades::Socialite::driver` / `contracts::Factory`, `Provider` and `User` expose the public contracts.

 Socialite requests use the shared client’s explicit `redactPayload()` mode: observers and recordings retain status and route but omit query, authorization, request body, response body and response headers. The actual network request and returned response remain complete. Provider status and malformed-response errors do not print response credentials.
-

 `ProviderEndpoints::new(authorize, token, user).email(email)` supplies explicit endpoint overrides to the new drivers’ `withEndpoints` constructors.

````
