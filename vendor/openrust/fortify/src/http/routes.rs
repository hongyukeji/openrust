use crate::{
    Fortify,
    http::{
        controllers::*,
        requests::{LoginRequest, TwoFactorLoginRequest},
    },
    resolver::Resolver,
};
use openrust::{
    http::Method,
    routing::{ControllerDispatcher, RouteGroup, RoutingError},
    support::facades::{Facade as _, Route},
};

pub(crate) fn register() -> Result<(), RoutingError> {
    let fortify = Resolver::<Fortify>::getFacadeRoot()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    let mut group = RouteGroup::new().prefix(fortify.string("fortify.prefix", ""));
    if let Some(domain) = fortify
        .config
        .get("fortify.domain")
        .and_then(|v| v.as_str().map(str::to_owned))
    {
        group = group.domain(domain);
    }
    let middleware = fortify
        .config
        .get("fortify.middleware")
        .unwrap_or_else(|| serde_json::json!(["web"]));
    let middleware = middleware.as_array().ok_or(RoutingError::InvalidAction)?;
    for value in middleware {
        group = group.middleware_name(value.as_str().ok_or(RoutingError::InvalidAction)?);
    }
    let guard = fortify.guard().unwrap_or_default();
    let guest = if guard.is_empty() {
        "guest".into()
    } else {
        format!("guest:{guard}")
    };
    let auth = if guard.is_empty() {
        fortify.string("fortify.auth_middleware", "auth")
    } else {
        format!(
            "{}:{guard}",
            fortify.string("fortify.auth_middleware", "auth")
        )
    };
    let views = fortify.truthy("fortify.views");
    let path = |name: &str, default: &str| {
        fortify
            .config
            .get(&format!("fortify.paths.{name}"))
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| default.to_owned())
    };
    Route::group(group, |router| {
        if views {
            router
                .r#match(
                    &[Method::GET],
                    &path("login", "/login"),
                    ControllerDispatcher::new(AuthenticatedSessionController::create),
                )?
                .middleware_name(guest.clone())
                .name("login");
        }
        let route = router.r#match(
            &[Method::POST],
            &path("login", "/login"),
            ControllerDispatcher::withFormRequest::<LoginRequest>(
                AuthenticatedSessionController::store,
            ),
        )?;
        route.middleware_name(guest.clone()).name("login.store");
        if fortify.truthy("fortify.limiters.login") {
            route.middleware_name(format!(
                "throttle:{}",
                fortify.string("fortify.limiters.login", "")
            ));
        }
        router
            .r#match(
                &[Method::POST],
                &path("logout", "/logout"),
                ControllerDispatcher::new(AuthenticatedSessionController::destroy),
            )?
            .middleware_name(auth.clone())
            .name("logout");
        if views {
            router
                .r#match(
                    &[Method::GET],
                    &path("password.confirm", "/user/confirm-password"),
                    ControllerDispatcher::new(ConfirmablePasswordController::show),
                )?
                .middleware_name(auth.clone())
                .name("password.confirm");
        }
        router
            .r#match(
                &[Method::GET],
                &path("password.confirmation", "/user/confirmed-password-status"),
                ControllerDispatcher::new(ConfirmedPasswordStatusController::show),
            )?
            .middleware_name(auth.clone())
            .name("password.confirmation");
        router
            .r#match(
                &[Method::POST],
                &path("password.confirm", "/user/confirm-password"),
                ControllerDispatcher::new(ConfirmablePasswordController::store),
            )?
            .middleware_name(auth.clone())
            .name("password.confirm.store");
        let features = crate::Features::new(fortify.config.clone());
        if features
            .enabled(crate::Features::twoFactorAuthentication())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            if views {
                router
                    .r#match(
                        &[Method::GET],
                        &path("two-factor.login", "/two-factor-challenge"),
                        ControllerDispatcher::new(TwoFactorAuthenticatedSessionController::create),
                    )?
                    .middleware_name(guest.clone())
                    .name("two-factor.login");
            }
            let route = router.r#match(
                &[Method::POST],
                &path("two-factor.login", "/two-factor-challenge"),
                ControllerDispatcher::withFormRequest::<TwoFactorLoginRequest>(
                    TwoFactorAuthenticatedSessionController::store,
                ),
            )?;
            route
                .middleware_name(guest.clone())
                .name("two-factor.login.store");
            if fortify.truthy("fortify.limiters.two-factor") {
                route.middleware_name(format!(
                    "throttle:{}",
                    fortify.string("fortify.limiters.two-factor", "")
                ));
            }
            let confirm = features
                .optionEnabled(
                    crate::Features::twoFactorAuthentication(),
                    "confirmPassword",
                )
                .map_err(|_| RoutingError::InvalidAction)?;
            macro_rules! management_route {
                ($method:expr,$key:expr,$default:expr,$action:expr,$name:expr) => {
                    let route = router.r#match(
                        &[$method],
                        &path($key, $default),
                        ControllerDispatcher::new($action),
                    )?;
                    route.middleware_name(auth.clone()).name($name);
                    if confirm {
                        route.middleware_name("password.confirm");
                    }
                };
            }
            management_route!(
                Method::POST,
                "two-factor.enable",
                "/user/two-factor-authentication",
                TwoFactorAuthenticationController::store,
                "two-factor.enable"
            );
            management_route!(
                Method::POST,
                "two-factor.confirm",
                "/user/confirmed-two-factor-authentication",
                ConfirmedTwoFactorAuthenticationController::store,
                "two-factor.confirm"
            );
            management_route!(
                Method::DELETE,
                "two-factor.disable",
                "/user/two-factor-authentication",
                TwoFactorAuthenticationController::destroy,
                "two-factor.disable"
            );
            management_route!(
                Method::GET,
                "two-factor.qr-code",
                "/user/two-factor-qr-code",
                TwoFactorQrCodeController::show,
                "two-factor.qr-code"
            );
            management_route!(
                Method::GET,
                "two-factor.secret-key",
                "/user/two-factor-secret-key",
                TwoFactorSecretKeyController::show,
                "two-factor.secret-key"
            );
            management_route!(
                Method::GET,
                "two-factor.recovery-codes",
                "/user/two-factor-recovery-codes",
                RecoveryCodeController::index,
                "two-factor.recovery-codes"
            );
            management_route!(
                Method::POST,
                "two-factor.recovery-codes",
                "/user/two-factor-recovery-codes",
                RecoveryCodeController::store,
                "two-factor.regenerate-recovery-codes"
            );
        }
        Ok(())
    })?;
    Ok(())
}
