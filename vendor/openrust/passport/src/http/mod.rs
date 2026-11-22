//! Browser consent and token endpoints. Application views keep responsibility for UI.
pub mod controllers;
pub mod device_controllers;
pub mod middleware;
pub mod transient_token_controller;
pub type DeviceUserCodeView =
    for<'a> fn(&'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>>;

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
    let device_enabled = passport
        .deviceCodeGrantEnabled
        .load(std::sync::atomic::Ordering::Relaxed);
    Route::group(group, |router| {
        router
            .post("/token", controllers::AccessTokenController::issueToken)?
            .name("token")
            .middleware_name("throttle");
        if device_enabled {
            router
                .post(
                    "/device/code",
                    device_controllers::DeviceCodeController::issue,
                )?
                .name("device.code")
                .middleware_name("throttle");
        }
        router.group(RouteGroup::new().middleware_name("web"), |router| {
            router
                .post(
                    "/token/refresh",
                    transient_token_controller::TransientTokenController::refresh,
                )?
                .name("token.refresh");
            if device_enabled {
                router
                    .get(
                        "/device",
                        device_controllers::DeviceUserCodeController::show,
                    )?
                    .name("device");
                router
                    .get(
                        "/device/authorize",
                        device_controllers::DeviceAuthorizationController::authorize,
                    )?
                    .name("device.authorizations.authorize")
                    .block(10, 10);
                router
                    .post(
                        "/device/authorize",
                        device_controllers::ApproveDeviceAuthorizationController::approve,
                    )?
                    .name("device.authorizations.approve")
                    .block(10, 10);
                router
                    .delete(
                        "/device/authorize",
                        device_controllers::DenyDeviceAuthorizationController::deny,
                    )?
                    .name("device.authorizations.deny")
                    .block(10, 10);
            }
            router
                .get(
                    "/authorize",
                    controllers::AuthorizationController::authorize,
                )?
                .name("authorizations.authorize")
                .block(10, 10);
            router
                .post(
                    "/authorize",
                    controllers::ApproveAuthorizationController::approve,
                )?
                .name("authorizations.approve")
                .block(10, 10);
            router
                .delete("/authorize", controllers::DenyAuthorizationController::deny)?
                .name("authorizations.deny")
                .block(10, 10);
            Ok(())
        })
    })
}
