//! 显式用户套件的网页适配；原生Fortify未装配时不改变控制器行为。
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    contracts::{
        auth::Guard as _, container::Container as _, foundation::Application as _,
        session::Session as _,
    },
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response},
    sanctum::HasApiTokens,
    support::{
        ProviderResult,
        facades::{App, Auth, Facade as _},
    },
};
use std::sync::Arc;

type Handler = for<'a> fn(&'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>>;
type Confirm = for<'a> fn(&'a mut Request) -> HttpFuture<'a, Result<(), HttpError>>;
struct Handlers {
    profile: Handler,
    password: Handler,
    confirm: Confirm,
}
const CONFIRMATION: &str = "auth.user_confirmation";

pub(super) fn register<M>(app: &Application) -> ProviderResult
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    app.container().instance(Arc::new(Handlers {
        profile: super::updates::web_profile::<M>,
        password: super::updates::web_password::<M>,
        confirm: record::<M>,
    }))?;
    Ok(())
}
async fn handlers() -> Result<Option<Arc<Handlers>>, HttpError> {
    let Some(app) = App::get_facade_application()? else {
        return Ok(None);
    };
    if !app
        .container()
        .has(openrust::container::ServiceKey::typed::<Handlers>())
    {
        return Ok(None);
    }
    Ok(Some(app.make::<Handlers>().await?))
}
pub(crate) async fn profile(request: &mut Request) -> Result<Option<Response>, HttpError> {
    match handlers().await? {
        Some(callback) => (callback.profile)(request).await.map(Some),
        None => Ok(None),
    }
}
pub(crate) async fn password(request: &mut Request) -> Result<Option<Response>, HttpError> {
    match handlers().await? {
        Some(callback) => (callback.password)(request).await.map(Some),
        None => Ok(None),
    }
}
pub(crate) async fn record_confirmation(request: &mut Request) -> Result<(), HttpError> {
    if let Some(callback) = handlers().await? {
        (callback.confirm)(request).await?;
    }
    Ok(())
}
pub(super) async fn user<M: HasApiTokens + Clone>(
    request: &mut Request,
    fortify: &Fortify,
) -> Result<M, HttpError> {
    let mut guard = Auth::session_guard(request, fortify.guard().as_deref())?;
    let user = guard.user().await?.ok_or(HttpError::Unauthenticated)?;
    (user as &dyn std::any::Any)
        .downcast_ref::<M>()
        .cloned()
        .ok_or(HttpError::Unauthenticated)
}
fn record<M>(request: &mut Request) -> HttpFuture<'_, Result<(), HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let user = user::<M>(request, &fortify).await?;
        remember(request, &user, &fortify)
    })
}
pub(super) fn remember<M: TwoFactorAuthenticatable>(
    request: &mut Request,
    user: &M,
    fortify: &Fortify,
) -> Result<(), HttpError> {
    let at = request
        .session()
        .map_err(internal)?
        .get("auth.password_confirmed_at")
        .cloned()
        .ok_or(HttpError::Unauthenticated)?;
    let binding = super::client::security(user, fortify)?;
    request
        .session_mut()
        .map_err(internal)?
        .put(CONFIRMATION, serde_json::json!({"at":at,"binding":binding}))
        .map_err(internal)
}
pub(super) fn confirmed<M: TwoFactorAuthenticatable>(
    request: &Request,
    user: &M,
    fortify: &Fortify,
) -> Result<bool, HttpError> {
    let session = request.session().map_err(internal)?;
    let Some(value) = session.get(CONFIRMATION) else {
        return Ok(false);
    };
    let current = super::client::security(user, fortify)?;
    Ok(value.get("at") == session.get("auth.password_confirmed_at")
        && value
            .get("binding")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|binding| super::client::same_binding(binding, &current)))
}
pub(super) fn forget(request: &mut Request) -> Result<(), HttpError> {
    request
        .session_mut()
        .map_err(internal)?
        .forget(&[CONFIRMATION, "auth.password_confirmed_at"]);
    Ok(())
}
