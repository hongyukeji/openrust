use crate::{
    Passkeys,
    http::{
        PasskeyConfirmationController, PasskeyLoginController, PasskeyRegistrationController,
        Resolver,
    },
};
use openrust::{
    http::Method,
    routing::{ControllerDispatcher, RouteGroup, RoutingError},
    support::facades::{Facade as _, Route},
};
pub(crate) fn register() -> Result<(), RoutingError> {
    let service = Resolver::<Passkeys>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    let mut group = RouteGroup::new();
    let middleware = service
        .config
        .get("passkeys.middleware")
        .unwrap_or(serde_json::json!(["web"]));
    for value in middleware.as_array().ok_or(RoutingError::InvalidAction)? {
        group = group.middleware_name(value.as_str().ok_or(RoutingError::InvalidAction)?);
    }
    let guard = service.string("passkeys.guard", "web");
    let management = service
        .config
        .get("passkeys.management_middleware")
        .unwrap_or(serde_json::json!(["password.confirm"]));
    Route::group(group, |router| {
        macro_rules! route {
            ($method:expr,$path:expr,$action:expr,$name:expr,$guest:expr,$manage:expr,$throttle:expr) => {{
                let route =
                    router.r#match(&[$method], $path, ControllerDispatcher::new($action))?;
                route
                    .middleware_name(format!("{}:{guard}", if $guest { "guest" } else { "auth" }))
                    .name($name);
                if $manage {
                    for middleware in management.as_array().ok_or(RoutingError::InvalidAction)? {
                        if let Some(value) = middleware.as_str().filter(|s| !s.is_empty()) {
                            route.middleware_name(value);
                        }
                    }
                }
                if $throttle
                    && let Some(value) = service
                        .config
                        .get("passkeys.throttle")
                        .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
                {
                    route.middleware_name(value);
                }
            }};
        }
        route!(
            Method::GET,
            "/passkeys/login/options",
            PasskeyLoginController::index,
            "passkey.login-options",
            true,
            false,
            true
        );
        route!(
            Method::POST,
            "/passkeys/login",
            PasskeyLoginController::store,
            "passkey.login",
            true,
            false,
            true
        );
        route!(
            Method::GET,
            "/passkeys/confirm/options",
            PasskeyConfirmationController::index,
            "passkey.confirm-options",
            false,
            false,
            true
        );
        route!(
            Method::POST,
            "/passkeys/confirm",
            PasskeyConfirmationController::store,
            "passkey.confirm",
            false,
            false,
            true
        );
        route!(
            Method::GET,
            "/user/passkeys/options",
            PasskeyRegistrationController::index,
            "passkey.registration-options",
            false,
            true,
            true
        );
        route!(
            Method::POST,
            "/user/passkeys",
            PasskeyRegistrationController::store,
            "passkey.store",
            false,
            true,
            true
        );
        route!(
            Method::DELETE,
            "/user/passkeys/{passkey}",
            PasskeyRegistrationController::destroy,
            "passkey.destroy",
            false,
            true,
            false
        );
        Ok(())
    })?;
    Ok(())
}
