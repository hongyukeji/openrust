# Socialite

Socialite connects third-party login to the application’s HTTP client and Session. The current accepted implementation covers GitHub and the shared OAuth2 flow against fixed Socialite 5.33.0. API-S01 remains in development; other providers and OAuth1/OIDC-specific paths are pending.

## Installation {#installation}

Enable the optional `socialite` Cargo feature when building or running the application. Package discovery registers `SocialiteServiceProvider` and the application-local `SocialiteManager`. It creates no routes or local user table automatically.

```sh
cargo run --features socialite -- artisan serve
```

## Configuration {#configuration}

Configure `services.github.client_id`, `client_secret` and `redirect` through `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `GITHUB_REDIRECT_URI`. A redirect beginning with `/` is appended to a valid absolute HTTP(S) `app.url`; an absolute callback is retained. `services.github.scopes` adds scopes to the driver defaults. Missing or invalid configuration fails explicitly. Use a registered GitHub OAuth application and its callback for real provider acceptance.

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
            .redirect().map_err(|_| HttpError::InvalidRequest)?;
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

## Optional Parameters {#optional-parameters}

Call `provider.with(serde_json::json!({"prompt":"select_account","login_hint":"alice@example.test"}).as_object().unwrap().clone())` before redirecting or exchanging a code. The values are merged after the ordinary fields, as in the fixed upstream implementation. Scalar strings, booleans and numbers are supported. `redirectUrl` changes this provider instance’s callback.

## Access Scopes {#access-scopes}

The GitHub default is `user:email`. `scopes(["read:user".into()])` merges unique scopes, while `setScopes(vec!["read:user".into()])` replaces them. `getScopes()` returns the current list. GitHub receives comma-separated scopes. When `user:email` is selected, Socialite queries `/user/emails` and selects a primary, verified email; failure or no match produces `None`. Removing that scope retains the profile’s email.

## Stateless Authentication {#stateless-authentication}

Call `provider.stateless()` before `user()` when the caller explicitly manages authentication state outside Session. This disables the state check; it does not turn a token into a local user or create a Session. PKCE still needs Session when enabled, matching the separate upstream controls.

## Retrieving User Details {#retrieving-user-details}

The user exposes `getId`, `getNickname`, `getName`, `getEmail`, `getAvatar`, `getRaw` and mapped `attributes` such as GitHub `nodeId`. The provider ID retains its JSON number/string type. `userFromToken(token)` retrieves a profile using an existing token. `refreshToken(token)` exchanges a refresh token. Credentials are explicit in `user.credentials`: `token`, `refreshToken`, `expiresIn` and `approvedScopes`; token strings use `Zeroizing<String>` and are not automatically serialized or printed by Debug. A remote access token is not a Passport or Sanctum credential.

## Testing {#testing}

Use a controlled HTTP provider with `GithubProvider::withEndpoints` and application-local `manager.extend("github", Arc::new(provider))`. It customizes authorization/token/profile/email URLs while retaining the driver behavior. `ProviderDefinition` supports compiled protocol customization. `facades::Socialite::driver` / `contracts::Factory`, `Provider` and `User` expose the public contracts. Tests execute real HTTP plus two independent Session applications, PKCE, state consumption, scope/parameter mapping, verified email, refresh and failure paths; fixed PHP source checks protect the comparison fixture.

Socialite requests use the shared client’s explicit `redactPayload()` mode: observers and recordings retain status and route but omit query, authorization, request body, response body and response headers. The actual network request and returned response remain complete. Provider status and malformed-response errors do not print response credentials. Controlled-provider acceptance does not claim real GitHub authorization; that still requires an OAuth application, credentials, callback domain and account.
