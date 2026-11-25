//! 浏览器设备页面与原生表单响应；和API共用撤销事务，保留CSRF及近期确认。
use super::{account, client, device_api, devices};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    contracts::{auth::Guard as _, session::Session as _},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    sanctum::HasApiTokens,
    session::SessionManager,
    support::facades::{App, Auth, Date, Facade as _, Redirect, Route, View},
};

pub(super) fn routes<M: HasApiTokens + TwoFactorAuthenticatable + Clone>()
-> Result<(), RoutingError> {
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    if !fortify.truthy("fortify.views")
        || !devices::enabled(fortify.config.as_ref()).map_err(|_| RoutingError::InvalidAction)?
    {
        return Ok(());
    }
    let guard = devices::policy().map_err(|_| RoutingError::InvalidAction)?;
    let guard = guard.ok_or(RoutingError::InvalidAction)?.guard.clone();
    let group = RouteGroup::new()
        .middleware_name("web")
        .middleware_name("auth.session")
        .middleware_name(format!("auth:{guard}"));
    Route::group(group, |router| {
        router
            .get("/user/sessions", index::<M>)?
            .name("user.sessions.page");
        router
            .delete("/user/sessions", destroy_others::<M>)?
            .name("user.sessions.web-destroy-others");
        router
            .delete("/user/sessions/{session}", destroy::<M>)?
            .name("user.sessions.web-destroy");
        Ok(())
    })
}
fn index<M: HasApiTokens + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let policy = devices::policy()?.ok_or(HttpError::NotFound)?;
        let user = account::user::<M>(request).await?;
        let manager = App::make::<SessionManager>().await?;
        let list = policy
            .list(
                &user,
                &manager,
                Some(request.session().map_err(internal)?.get_id()),
            )
            .await?;
        let timezone = policy
            .config
            .get("app.timezone")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "UTC".into());
        let zone = openrust::support::Date::timezone(&timezone).map_err(internal)?;
        let mut data = Vec::new();
        for device in list {
            let label = Date::create_from_timestamp(device.last_active, Some(zone.clone()))
                .map_err(internal)?
                .format("Y-m-d H:i:s T")
                .map_err(internal)?;
            let mut value = serde_json::to_value(device).map_err(internal)?;
            value["last_active_label"] = label.into();
            data.push(value);
        }
        let view = View::make(
            "user.sessions",
            [("devices".into(), serde_json::Value::Array(data).into())].into(),
        )?;
        Response::view(StatusCode::OK, view, Default::default(), request)
            .await
            .map(client::no_store)
    })
}
fn destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    remove::<M>(request, false)
}
fn destroy_others<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    remove::<M>(request, true)
}
fn remove<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    others: bool,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let policy = devices::policy()?.ok_or(HttpError::NotFound)?;
        let response = device_api::revoke_browser::<M>(request, others).await?;
        if request.expects_json() {
            return Ok(response);
        }
        let logged_in = Auth::session_guard(request, Some(&policy.guard))?
            .check()
            .await?;
        let route = if logged_in {
            "user.sessions.page"
        } else {
            "login"
        };
        Ok(client::no_store(
            Redirect::route(route, &[], Default::default())?
                .with(request, "status", "user.sessions_updated".into())?
                .into(),
        ))
    })
}
