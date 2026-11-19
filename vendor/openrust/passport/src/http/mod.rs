//! Browser consent and token endpoints. Application views keep responsibility for UI.
pub mod controllers;

use crate::{AuthorizationRequest, ClientRecord, Scope};
use openrust::{
    contracts::auth::Authenticatable,
    http::{HttpError, HttpFuture, Request, Response},
};
use std::sync::Arc;

/// Native counterpart of Passport::authorizationView's closure.
pub type AuthorizationView = for<'a> fn(
    &'a mut Request,
    AuthorizationViewParameters,
) -> HttpFuture<'a, Result<Response, HttpError>>;
pub struct AuthorizationViewParameters {
    pub client: ClientRecord,
    pub user: Arc<dyn Authenticatable>,
    pub scopes: Vec<Scope>,
    pub auth_token: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PendingAuthorization {
    request: AuthorizationRequest,
    user: openrust::auth::AuthIdentifier,
}

pub(crate) fn routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::{
        contracts::container::Container as _,
        routing::RouteGroup,
        support::facades::{Config, Route},
    };
    let passport = openrust::container::Container::getInstance()
        .make::<crate::Passport>()
        .map_err(|_| openrust::routing::RoutingError::InvalidAction)?;
    if !passport
        .registersRoutes
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Ok(());
    }
    let prefix = Config::get("passport.path")
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "oauth".into());
    let mut group = RouteGroup::new().prefix(prefix).name("passport.");
    if let Some(serde_json::Value::Array(middleware)) = Config::get("passport.middleware")
        .map_err(|_| openrust::routing::RoutingError::InvalidAction)?
    {
        for middleware in middleware {
            group = group.middleware_name(
                middleware
                    .as_str()
                    .ok_or(openrust::routing::RoutingError::InvalidAction)?,
            );
        }
    }
    Route::group(group, |router| {
        router
            .r#match(
                &[openrust::http::Method::POST],
                "/token",
                controllers::AccessTokenController::issueToken,
            )?
            .name("token")
            .middleware_name("throttle");
        router.group(RouteGroup::new().middleware_name("web"), |router| {
            router
                .get(
                    "/authorize",
                    controllers::AuthorizationController::authorize,
                )?
                .name("authorizations.authorize")
                .block(10, 10);
            router
                .r#match(
                    &[openrust::http::Method::POST],
                    "/authorize",
                    controllers::ApproveAuthorizationController::approve,
                )?
                .name("authorizations.approve")
                .block(10, 10);
            router
                .r#match(
                    &[openrust::http::Method::DELETE],
                    "/authorize",
                    controllers::DenyAuthorizationController::deny,
                )?
                .name("authorizations.deny")
                .block(10, 10);
            Ok(())
        })
    })
}
