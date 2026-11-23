use crate::{
    Fortify,
    http::{
        controllers::*,
        requests::{LoginRequest, SendPasswordResetLinkRequest, TwoFactorLoginRequest},
    },
    resolver::Resolver,
};
use openrust::{
    http::Method,
    routing::{ControllerDispatcher, RouteGroup, RoutingError},
    support::facades::{Facade as _, Route},
};

pub(crate) fn register() -> Result<(), RoutingError> {
    let fortify = Resolver::<Fortify>::get_facade_root()
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
                .get(
                    &path("login", "/login"),
                    ControllerDispatcher::new(AuthenticatedSessionController::create),
                )?
                .middleware_name(guest.clone())
                .name("login");
        }
        let route = router.post(
            &path("login", "/login"),
            ControllerDispatcher::with_form_request::<LoginRequest>(
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
            .post(
                &path("logout", "/logout"),
                ControllerDispatcher::new(AuthenticatedSessionController::destroy),
            )?
            .middleware_name(auth.clone())
            .name("logout");
        if views {
            router
                .get(
                    &path("password.confirm", "/user/confirm-password"),
                    ControllerDispatcher::new(ConfirmablePasswordController::show),
                )?
                .middleware_name(auth.clone())
                .name("password.confirm");
        }
        router
            .get(
                &path("password.confirmation", "/user/confirmed-password-status"),
                ControllerDispatcher::new(ConfirmedPasswordStatusController::show),
            )?
            .middleware_name(auth.clone())
            .name("password.confirmation");
        router
            .post(
                &path("password.confirm", "/user/confirm-password"),
                ControllerDispatcher::new(ConfirmablePasswordController::store),
            )?
            .middleware_name(auth.clone())
            .name("password.confirm.store");
        let features = crate::Features::new(fortify.config.clone());
        if features
            .enabled(crate::Features::resetPasswords())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            if views {
                router
                    .get(
                        &path("password.request", "/forgot-password"),
                        ControllerDispatcher::new(PasswordResetLinkController::create),
                    )?
                    .middleware_name(guest.clone())
                    .name("password.request");
                router
                    .get(
                        &path("password.reset", "/reset-password/{token}"),
                        ControllerDispatcher::new(NewPasswordController::create),
                    )?
                    .middleware_name(guest.clone())
                    .name("password.reset");
            }
            router
                .post(
                    &path("password.email", "/forgot-password"),
                    ControllerDispatcher::with_form_request::<SendPasswordResetLinkRequest>(
                        PasswordResetLinkController::store,
                    ),
                )?
                .middleware_name(guest.clone())
                .name("password.email");
            router
                .post(
                    &path("password.update", "/reset-password"),
                    ControllerDispatcher::new(NewPasswordController::store),
                )?
                .middleware_name(guest.clone())
                .name("password.update");
        }
        if features
            .enabled(crate::Features::emailVerification())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            if views {
                router
                    .get(
                        &path("verification.notice", "/email/verify"),
                        ControllerDispatcher::new(EmailVerificationPromptController::show),
                    )?
                    .middleware_name(auth.clone())
                    .name("verification.notice");
            }
            let limiter = fortify.string("fortify.limiters.verification", "6,1");
            router
                .get(
                    &path("verification.verify", "/email/verify/{id}/{hash}"),
                    ControllerDispatcher::with_form_request::<
                        crate::http::requests::VerifyEmailRequest,
                    >(VerifyEmailController::verify),
                )?
                .middleware_name(auth.clone())
                .middleware_name("signed")
                .middleware_name(format!("throttle:{limiter}"))
                .name("verification.verify");
            router
                .post(
                    &path("verification.send", "/email/verification-notification"),
                    ControllerDispatcher::new(EmailVerificationNotificationController::store),
                )?
                .middleware_name(auth.clone())
                .middleware_name(format!("throttle:{limiter}"))
                .name("verification.send");
        }
        if features
            .enabled(crate::Features::registration())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            if views {
                router
                    .get(
                        &path("register", "/register"),
                        ControllerDispatcher::new(RegisteredUserController::create),
                    )?
                    .middleware_name(guest.clone())
                    .name("register");
            }
            router
                .post(
                    &path("register", "/register"),
                    ControllerDispatcher::new(RegisteredUserController::store),
                )?
                .middleware_name(guest.clone())
                .name("register.store");
        }
        if features
            .enabled(crate::Features::updateProfileInformation())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            router
                .put(
                    &path(
                        "user-profile-information.update",
                        "/user/profile-information",
                    ),
                    ControllerDispatcher::new(ProfileInformationController::update),
                )?
                .middleware_name(auth.clone())
                .name("user-profile-information.update");
        }
        if features
            .enabled(crate::Features::updatePasswords())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            router
                .put(
                    &path("user-password.update", "/user/password"),
                    ControllerDispatcher::new(PasswordController::update),
                )?
                .middleware_name(auth.clone())
                .name("user-password.update");
        }
        if features
            .enabled(crate::Features::passkeys())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            use openrust_passkeys::http::{
                PasskeyConfirmationController, PasskeyLoginController,
                PasskeyRegistrationController,
            };
            let confirm = fortify
                .config
                .get("fortify-options.passkeys.confirmPassword")
                .unwrap_or(true.into());
            let confirm = confirm.as_bool().unwrap_or(true);
            macro_rules! passkey_route {
                ($method:expr,$name:expr,$path:expr,$action:expr,$guest:expr,$manage:expr,$throttle:expr) => {{
                    let route = router.r#match(
                        &[$method],
                        &path($name, $path),
                        ControllerDispatcher::new($action),
                    )?;
                    route
                        .middleware_name(if $guest { guest.clone() } else { auth.clone() })
                        .name($name);
                    if $manage && confirm {
                        route.middleware_name("password.confirm");
                    }
                    if $throttle && fortify.truthy("fortify.limiters.passkeys") {
                        route.middleware_name(format!(
                            "throttle:{}",
                            fortify.string("fortify.limiters.passkeys", "")
                        ));
                    }
                }};
            }
            passkey_route!(
                Method::GET,
                "passkey.login-options",
                "/passkeys/login/options",
                PasskeyLoginController::index,
                true,
                false,
                true
            );
            passkey_route!(
                Method::POST,
                "passkey.login",
                "/passkeys/login",
                PasskeyLoginController::store,
                true,
                false,
                true
            );
            passkey_route!(
                Method::GET,
                "passkey.confirm-options",
                "/passkeys/confirm/options",
                PasskeyConfirmationController::index,
                false,
                false,
                true
            );
            passkey_route!(
                Method::POST,
                "passkey.confirm",
                "/passkeys/confirm",
                PasskeyConfirmationController::store,
                false,
                false,
                true
            );
            passkey_route!(
                Method::GET,
                "passkey.registration-options",
                "/user/passkeys/options",
                PasskeyRegistrationController::index,
                false,
                true,
                true
            );
            passkey_route!(
                Method::POST,
                "passkey.store",
                "/user/passkeys",
                PasskeyRegistrationController::store,
                false,
                true,
                true
            );
            passkey_route!(
                Method::DELETE,
                "passkey.destroy",
                "/user/passkeys/{passkey}",
                PasskeyRegistrationController::destroy,
                false,
                true,
                false
            );
        }
        if features
            .enabled(crate::Features::twoFactorAuthentication())
            .map_err(|_| RoutingError::InvalidAction)?
        {
            if views {
                router
                    .get(
                        &path("two-factor.login", "/two-factor-challenge"),
                        ControllerDispatcher::new(TwoFactorAuthenticatedSessionController::create),
                    )?
                    .middleware_name(guest.clone())
                    .name("two-factor.login");
            }
            let route = router.post(
                &path("two-factor.login", "/two-factor-challenge"),
                ControllerDispatcher::with_form_request::<TwoFactorLoginRequest>(
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
