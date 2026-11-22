#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use crate::{AccessToken, ClientRepository, ResourceServer};
use openrust::{
    auth::{AuthError, AuthFuture, AuthIdentifier, AuthManager, AuthResult},
    contracts::{auth::Authenticatable, container::Container as _},
    http::Request,
    support::facades::Facade,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct RequestTokens(BTreeMap<String, AccessToken>);
/// Bearer-token user resolution. State is isolated by request and guard name.
pub struct TokenGuard;
impl TokenGuard {
    #[allow(non_snake_case)]
    pub fn currentToken(request: &Request) -> Option<&AccessToken> {
        let guard = openrust::auth::Auth::guardName(request, None).ok()?;
        Self::token(request, &guard)
    }

    pub(crate) fn store(request: &mut Request, guard: &str, token: AccessToken) {
        if request.parts().extensions.get::<RequestTokens>().is_none() {
            request
                .parts_mut()
                .extensions
                .insert(RequestTokens::default());
        }
        request
            .parts_mut()
            .extensions
            .get_mut::<RequestTokens>()
            .expect("inserted tokens")
            .0
            .insert(guard.into(), token);
    }
    pub fn token<'a>(request: &'a Request, guard: &str) -> Option<&'a AccessToken> {
        request
            .parts()
            .extensions
            .get::<RequestTokens>()?
            .0
            .get(guard)
    }
    pub(crate) fn resolve<'a>(
        request: &'a mut Request,
        guard: &'a str,
        config: &'a Value,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async move {
            if let Some(tokens) = request.parts_mut().extensions.get_mut::<RequestTokens>() {
                tokens.0.remove(guard);
            }
            request.parts_mut().extensions.remove::<AccessToken>();
            if request.bearerToken()?.is_none() {
                let container = openrust::container::Container::getInstance();
                let passport = container.make::<crate::Passport>().map_err(backend)?;
                if !request.hasCookie(&passport.cookie(None))? {
                    return Ok(None);
                }
                let encrypter = container
                    .make::<openrust::encryption::Encrypter>()
                    .map_err(backend)?;
                let user_id = match crate::api_token_cookie_factory::authenticate(
                    request, &passport, &encrypter,
                ) {
                    Ok(Some(id)) => id,
                    _ => return Ok(None),
                };
                let provider = config
                    .get("provider")
                    .and_then(Value::as_str)
                    .ok_or(AuthError::Configuration("Passport provider"))?;
                let user = container
                    .make::<AuthManager>()
                    .map_err(backend)?
                    .createUserProvider(provider)?
                    .retrieveById(&user_id)
                    .await
                    .unwrap_or(None);
                if user.is_some() {
                    Self::store(
                        request,
                        guard,
                        crate::api_token_cookie_factory::transient(user_id),
                    );
                }
                return Ok(user);
            }
            let server = ResourceServer::getFacadeRoot().map_err(backend)?;
            let token = match server.validateAuthenticatedRequest(request).await {
                Ok(token) => token,
                Err(error) if error.is_server_error() => return Err(backend(error)),
                Err(_) => {
                    request.parts_mut().headers.remove("authorization");
                    return Ok(None);
                }
            };
            let container = openrust::container::Container::getInstance();
            let clients = container.make::<ClientRepository>().map_err(backend)?;
            let Some(client) = clients
                .findActive(token.oauth_client_id.as_str())
                .await
                .map_err(backend)?
            else {
                return Ok(None);
            };
            let provider = config
                .get("provider")
                .and_then(Value::as_str)
                .ok_or(AuthError::Configuration("Passport provider"))?;
            if matches!(client.getAttributeValue("provider").await.map_err(backend)?,Some(openrust::database::Value::Text(name)) if !name.is_empty() && name != provider)
            {
                return Ok(None);
            }
            let Some(id) = token
                .oauth_user_id
                .as_deref()
                .filter(|v| !v.is_empty() && *v != "0")
            else {
                return Ok(None);
            };
            if id == token.oauth_client_id
                && client
                    .hasGrantType("client_credentials")
                    .await
                    .map_err(backend)?
            {
                return Ok(None);
            }
            let manager = container.make::<AuthManager>().map_err(backend)?;
            let user = manager
                .createUserProvider(provider)?
                .retrieveById(&AuthIdentifier::from(id))
                .await
                .unwrap_or(None);
            if user.is_some() {
                Self::store(request, guard, token);
            }
            Ok(user)
        })
    }
}
fn backend(error: impl std::error::Error + Send + Sync + 'static) -> AuthError {
    AuthError::Backend(Box::new(error))
}
