use crate::{Fortify, http::responses::SimpleViewResponse};
use openrust::{
    foundation::Application,
    http::{Response, StatusCode},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade as _, View},
    },
};
use std::sync::Arc;

/// 装配已发布的应用视图；API预设只注册邮件落地所需的缺失页面入口。
pub async fn register_views(app: &Application) -> ProviderResult {
    super::messages::register(app).await?;
    let fortify = app.make::<Fortify>().await?;
    fortify.login_view(if fortify.truthy("fortify.views") {
        "user.auth.login"
    } else {
        "user.auth.mail-login"
    });
    fortify.register_view("user.auth.register");
    fortify.confirm_password_view(SimpleViewResponse::callback(Arc::new(|request| {
        Box::pin(async move {
            let has_local_password = local_password(request).await?;
            let view = openrust::support::facades::View::make(
                "user.auth.confirm-password",
                [("has_local_password".into(), has_local_password.into())].into(),
            )?;
            Response::view(StatusCode::OK, view, Default::default(), request).await
        })
    })));
    fortify.request_password_reset_link_view("user.auth.forgot-password");
    fortify.verify_email_view("user.auth.verify-email");
    fortify.two_factor_challenge_view("user.auth.two-factor-challenge");
    fortify.reset_password_view(SimpleViewResponse::callback(Arc::new(|request| {
        Box::pin(async move {
            let token = request.route("token").unwrap_or_default().to_owned();
            let email = request
                .input("email")
                .await?
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            let view = View::make(
                "user.auth.reset-password",
                [
                    ("token".into(), token.into()),
                    ("email".into(), email.into()),
                ]
                .into(),
            )?;
            Response::view(StatusCode::OK, view, Default::default(), request).await
        })
    })));
    if fortify.truthy("fortify.views") {
        super::StarterKitServiceProvider.load_routes_from(app, account_routes)?;
    } else {
        super::StarterKitServiceProvider.load_routes_from(app, fallback_routes)?;
    }
    Ok(())
}
fn fallback_routes() -> Result<(), openrust::routing::RoutingError> {
    use crate::{
        http::controllers::{AuthenticatedSessionController, NewPasswordController},
        resolver::Resolver,
    };
    use openrust::{
        routing::{ControllerDispatcher, RouteGroup},
        support::facades::{Facade as _, Route},
    };
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?;
    let mut group = RouteGroup::new()
        .prefix(fortify.string("fortify.prefix", ""))
        .middleware_name("web");
    if let Some(domain) = fortify
        .config
        .get("fortify.domain")
        .and_then(|value| value.as_str().map(str::to_owned))
    {
        group = group.domain(domain);
    }
    Route::group(group, |router| {
        router
            .get(
                &fortify.string("fortify.paths.login", "/login"),
                ControllerDispatcher::new(AuthenticatedSessionController::create),
            )?
            .middleware_name(format!(
                "guest:{}",
                fortify.guard().as_deref().unwrap_or("web")
            ))
            .name("login");
        if crate::Features::new(fortify.config.clone())
            .enabled(crate::Features::email_verification())
            .map_err(|_| openrust::routing::RoutingError::InvalidAction)?
        {
            router
                .get(
                    &fortify.string("fortify.paths.verification.notice", "/email/verify"),
                    email_receipt,
                )?
                .middleware_name(format!(
                    "auth:{}",
                    fortify.guard().as_deref().unwrap_or("web")
                ))
                .name("verification.notice");
        }
        router
            .get(
                &fortify.string("fortify.paths.password.reset", "/reset-password/{token}"),
                ControllerDispatcher::new(NewPasswordController::create),
            )?
            .name("password.reset");
        Ok(())
    })
}

fn account_routes() -> Result<(), openrust::routing::RoutingError> {
    use crate::resolver::Resolver;
    use openrust::{
        routing::RouteGroup,
        support::facades::{Facade as _, Route},
    };
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?;
    let group = RouteGroup::new()
        .middleware_name("web")
        .middleware_name("auth.session")
        .middleware_name(format!(
            "auth:{}",
            fortify.guard().as_deref().unwrap_or("web")
        ));
    Route::group(group, |router| {
        router.get("/user/profile", account)?.name("user.profile");
        if fortify.string("fortify.home", "/home") == "/home" {
            router.get("/home", account)?.name("dashboard");
        }
        Ok(())
    })
}
fn account(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let has_local_password = local_password(request).await?;
        let user = super::UserResource::current_default(request).await?;
        let fortify = crate::resolver::Resolver::<Fortify>::get_facade_root()
            .map_err(crate::resolver::internal)?;
        let features = crate::Features::new(fortify.config.clone());
        #[cfg(feature = "sanctum")]
        let can_manage_devices = super::devices::policy()?.is_some();
        #[cfg(not(feature = "sanctum"))]
        let can_manage_devices = false;
        #[cfg(feature = "sanctum")]
        let (can_manage_avatar, avatar_limits) = (
            super::avatar::enabled(fortify.config.as_ref())?,
            super::avatar::view_limits(fortify.config.as_ref())?,
        );
        #[cfg(not(feature = "sanctum"))]
        let (can_manage_avatar, avatar_limits) = (false, (0usize, 0u32));
        let view = View::make(
            "user.home",
            [
                ("user".into(), user.into()),
                ("has_local_password".into(), has_local_password.into()),
                ("can_manage_devices".into(), can_manage_devices.into()),
                (
                    "can_manage_two_factor".into(),
                    (cfg!(feature = "sanctum")
                        && features
                            .enabled(crate::Features::two_factor_authentication())
                            .map_err(crate::resolver::internal)?)
                    .into(),
                ),
                ("can_manage_tokens".into(), cfg!(feature = "sanctum").into()),
                ("can_manage_avatar".into(), can_manage_avatar.into()),
                (
                    "can_delete_account".into(),
                    cfg!(feature = "sanctum").into(),
                ),
                ("avatar_max_kilobytes".into(), avatar_limits.0.into()),
                ("avatar_max_dimension".into(), avatar_limits.1.into()),
                (
                    "can_update_profile".into(),
                    features
                        .enabled(crate::Features::update_profile_information())
                        .map_err(crate::resolver::internal)?
                        .into(),
                ),
                (
                    "can_update_password".into(),
                    features
                        .enabled(crate::Features::update_passwords())
                        .map_err(crate::resolver::internal)?
                        .into(),
                ),
            ]
            .into(),
        )?;
        Response::view(StatusCode::OK, view, Default::default(), request).await
    })
}

fn email_receipt(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let user = super::UserResource::current_default(request).await?;
        let view = View::make(
            "user.auth.verify-email",
            [(
                "verified".into(),
                (!user["email_verified_at"].is_null()).into(),
            )]
            .into(),
        )?;
        Response::view(StatusCode::OK, view, Default::default(), request).await
    })
}

async fn local_password(
    request: &mut openrust::http::Request,
) -> Result<bool, openrust::http::HttpError> {
    use openrust::contracts::auth::Guard as _;
    let fortify = crate::resolver::Resolver::<Fortify>::get_facade_root()
        .map_err(crate::resolver::internal)?;
    let mut guard = openrust::auth::Auth::session_guard(request, fortify.guard().as_deref())?;
    Ok(guard
        .user()
        .await?
        .ok_or(openrust::http::HttpError::Unauthenticated)?
        .get_auth_password()
        .is_some())
}
