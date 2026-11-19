//! Fixed Passport CheckToken family; compiled scopes replace PHP middleware strings.
mod create_fresh_api_token;
use crate::{AccessToken, ResourceServer, guards::TokenGuard};
pub use create_fresh_api_token::CreateFreshApiToken;
use openrust::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    support::facades::{Auth, Facade},
};
use std::sync::Arc;
#[derive(Clone, Debug, thiserror::Error)]
#[error("Invalid scope(s) provided.")]
pub struct MissingScopeException {
    pub scopes: Vec<String>,
}
#[derive(Clone, Copy)]
enum Rule {
    All,
    Any,
    Client,
}
async fn validate(request: &mut Request) -> Result<AccessToken, HttpError> {
    if let Some(token) = request
        .parts()
        .extensions
        .get::<crate::testing::TestingToken>()
    {
        return Ok(token.0.clone());
    }
    // Resolve the selected user guard first. Machine tokens continue without a user.
    if Auth::user(request).await?.is_some()
        && let Some(token) = TokenGuard::currentToken(request)
    {
        return Ok(token.clone());
    }
    let server = ResourceServer::getFacadeRoot().map_err(|source| HttpError::Internal {
        source: Box::new(source),
    })?;
    server
        .validateAuthenticatedRequest(request)
        .await
        .map_err(|_| HttpError::Unauthenticated)
}
async fn check(request: &mut Request, scopes: &[String], rule: Rule) -> Result<(), HttpError> {
    let token = validate(request).await?;
    if matches!(rule, Rule::Client)
        && !token.transient()
        && token
            .oauth_user_id
            .as_ref()
            .is_some_and(|id| id != &token.oauth_client_id)
    {
        return Err(HttpError::Unauthenticated);
    }
    let mut missing = Vec::new();
    for scope in scopes {
        if token.can(scope).map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })? {
            if matches!(rule, Rule::Any) {
                return Ok(());
            }
        } else {
            missing.push(scope.clone());
        }
    }
    if matches!(rule, Rule::Any) || !missing.is_empty() {
        return Err(openrust::http::abort(
            StatusCode::FORBIDDEN,
            "Invalid scope(s) provided.",
            Default::default(),
        ));
    }
    Ok(())
}
macro_rules! middleware {
    ($name:ident,$rule:expr) => {
        pub struct $name {
            scopes: Vec<String>,
        }
        impl $name {
            pub fn using(scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
                Self {
                    scopes: scopes.into_iter().map(Into::into).collect(),
                }
            }
            pub fn shared(scopes: impl IntoIterator<Item = impl Into<String>>) -> Arc<Self> {
                Arc::new(Self::using(scopes))
            }
        }
        impl Middleware for $name {
            fn handle<'a>(
                &'a self,
                request: &'a mut Request,
                next: Next<'a>,
            ) -> HttpFuture<'a, Result<Response, HttpError>> {
                Box::pin(async move {
                    check(request, &self.scopes, $rule).await?;
                    Ok(next.run(request).await)
                })
            }
        }
    };
}
middleware!(CheckToken, Rule::All);
middleware!(CheckTokenForAnyScope, Rule::Any);
middleware!(EnsureClientIsResourceOwner, Rule::Client);
