//! 套件双因素管理：原生Action、用户共同写锁与当前客户端近期证明。
use super::{
    account, client,
    confirmation::{self, Purpose},
    credentials, updates,
};
use crate::{
    Features, Fortify, TwoFactorAuthenticatable,
    actions::{
        ConfirmTwoFactorAuthentication, DisableTwoFactorAuthentication,
        EnableTwoFactorAuthentication, GenerateNewRecoveryCodes,
    },
    contracts,
    resolver::{Resolver, internal},
    two_factor_model::Mutation,
};
use openrust::{
    auth::AuthError,
    contracts::{
        auth::Guard as _, container::Container as _, foundation::Application as _,
        session::Session as _,
    },
    database::DatabaseManager,
    foundation::Application,
    http::{ApiResult, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    sanctum::{HasApiTokens, Sanctum},
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Auth, Facade as _, Route, View},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Clone, Copy)]
pub(crate) enum Read {
    Qr,
    Secret,
    Codes,
    Setup,
}
enum Operation {
    Enable(bool),
    Confirm(String),
    Disable,
    Generate,
}
impl Operation {
    fn from_native(mutation: Mutation<'_>) -> Result<Self, HttpError> {
        Ok(match mutation {
            Mutation::Enable(force) => Self::Enable(force),
            Mutation::Confirm(code) => Self::Confirm(code.into()),
            Mutation::Disable => Self::Disable,
            Mutation::Generate => Self::Generate,
            Mutation::Consume(_) => {
                return Err(internal(std::io::Error::other(
                    "恢复码认证消费不能通过管理入口执行",
                )));
            }
        })
    }
}
type Mutate = for<'a> fn(&'a mut Request, Mutation<'a>) -> HttpFuture<'a, Result<(), HttpError>>;
type ReadHandler = for<'a> fn(&'a mut Request, Read) -> HttpFuture<'a, Result<Response, HttpError>>;
struct Handlers {
    mutate: Mutate,
    read: ReadHandler,
}
pub(super) fn register<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    app: &Application,
) -> ProviderResult {
    super::StarterKitServiceProvider.load_routes_from(app, routes::<M>)?;
    app.container().instance(Arc::new(Handlers {
        mutate: web_mutate::<M>,
        read: web_read::<M>,
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
pub(crate) async fn mutate_web<'a>(
    request: &'a mut Request,
    mutation: Mutation<'a>,
) -> Result<bool, HttpError> {
    let Some(handlers) = handlers().await? else {
        return Ok(false);
    };
    (handlers.mutate)(request, mutation).await?;
    Ok(true)
}
pub(crate) async fn read_web(
    request: &mut Request,
    kind: Read,
) -> Result<Option<Response>, HttpError> {
    match handlers().await? {
        Some(handlers) => (handlers.read)(request, kind).await.map(Some),
        None => Ok(None),
    }
}
fn enabled(fortify: &Fortify) -> Result<(), HttpError> {
    if !Features::new(fortify.config.clone())
        .enabled(Features::two_factor_authentication())
        .map_err(internal)?
    {
        return Err(HttpError::NotFound);
    }
    Ok(())
}
fn overview<M: TwoFactorAuthenticatable>(user: &M, fortify: &Fortify) -> Result<Value, HttpError> {
    let present = crate::two_factor_authenticatable::text(user, "two_factor_secret")
        .map_err(internal)?
        .is_some();
    let active = user
        .has_enabled_two_factor_authentication(fortify)
        .map_err(internal)?;
    Ok(json!({"enabled":active,"confirmation_required":present&&!active,"has_secret":present}))
}
fn data<M: TwoFactorAuthenticatable>(
    user: &M,
    fortify: &Fortify,
    provider: &dyn contracts::TwoFactorAuthenticationProvider,
    kind: Read,
) -> Result<Value, HttpError> {
    let encrypted =
        crate::two_factor_authenticatable::text(user, "two_factor_secret").map_err(internal)?;
    let Some(encrypted) = encrypted else {
        return if matches!(kind, Read::Secret) {
            Err(HttpError::NotFound)
        } else {
            Ok(json!([]))
        };
    };
    if matches!(kind, Read::Codes) {
        return Ok(json!({"recovery_codes":user.recovery_codes(fortify).map_err(internal)?}));
    }
    let secret = Zeroizing::new(
        fortify
            .current_encrypter()
            .map_err(internal)?
            .decrypt(&encrypted, true)
            .map_err(internal)?,
    );
    let secret = std::str::from_utf8(&secret).map_err(internal)?;
    if matches!(kind, Read::Secret) {
        return Ok(json!({"secretKey":secret}));
    }
    let svg = user
        .two_factor_qr_code_svg(fortify, provider)
        .map_err(internal)?;
    let url = user
        .two_factor_qr_code_url(fortify, provider)
        .map_err(internal)?;
    if matches!(kind, Read::Qr) {
        return Ok(json!({"svg":svg,"url":url}));
    }
    let mut result = overview(user, fortify)?;
    result["svg"] = svg.into();
    result["url"] = url.into();
    result["secretKey"] = secret.into();
    result["recovery_codes"] =
        serde_json::to_value(user.recovery_codes(fortify).map_err(internal)?).map_err(internal)?;
    Ok(result)
}
async fn change<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    operation: Operation,
    web: bool,
) -> Result<(M, Arc<Fortify>, Option<Value>), HttpError> {
    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
    enabled(&fortify)?;
    let mut user = if web {
        super::web::user::<M>(request, &fortify).await?
    } else {
        account::user::<M>(request).await?
    };
    let actor = if web {
        None
    } else {
        account::actor(request, &user, Some("account:update"))?
    };
    let guard = updates::cookie_guard(request, &user, actor.is_none(), &fortify).await?;
    let proof = if web {
        confirmation::claim_session(request, &user, &fortify).await?
    } else {
        confirmation::claim(request, &user, Purpose::Factors, &fortify).await?
    };
    let enable = App::make::<EnableTwoFactorAuthentication>().await?;
    let confirm = App::make::<ConfirmTwoFactorAuthentication>().await?;
    let disable = App::make::<DisableTwoFactorAuthentication>().await?;
    let generate = App::make::<GenerateNewRecoveryCodes>().await?;
    let sanctum = App::make::<Sanctum>().await?;
    let database = App::make::<DatabaseManager>().await?;
    let policy = fortify.clone();
    let enabling = matches!(operation, Operation::Enable(_));
    let ending = matches!(operation, Operation::Confirm(_) | Operation::Disable);
    let previous_secret =
        crate::two_factor_authenticatable::text(&user, "two_factor_secret").map_err(internal)?;
    let setup = enabling && !web;
    let codes = matches!(operation, Operation::Generate) && !web;
    let presentation = App::make::<dyn contracts::TwoFactorAuthenticationProvider>().await?;
    let payload = credentials::with_locked(&mut user, move |locked, tx| {
        Box::pin(async move {
            proof.validate_in_transaction(locked, &policy, tx).await?;
            account::check_actor(
                locked,
                tx,
                &sanctum,
                &database,
                actor.clone(),
                Some("account:update"),
                &[],
            )
            .await?;
            let before = locked.clone();
            let stamp = credentials::stamp(locked)?;
            match operation {
                Operation::Enable(force) => enable.call_in_transaction(locked, tx, force).await?,
                Operation::Confirm(code) => confirm.call_in_transaction(locked, tx, &code).await?,
                Operation::Disable => disable.call_in_transaction(locked, tx).await?,
                Operation::Generate => generate.call_in_transaction(locked, tx).await?,
            }
            locked.refresh_for_update(tx).await?;
            if stamp != credentials::stamp(locked)? {
                return Err(AuthError::from(HttpError::Unauthenticated));
            }
            account::check_actor(
                locked,
                tx,
                &sanctum,
                &database,
                actor,
                Some("account:update"),
                &[],
            )
            .await?;
            let payload = if setup {
                Some(data(locked, &policy, presentation.as_ref(), Read::Setup)?)
            } else if codes {
                Some(json!({"recovery_codes":locked.recovery_codes(&policy).map_err(internal)?}))
            } else {
                None
            };
            proof.validate_in_transaction(&before, &policy, tx).await?;
            Ok(payload)
        })
    })
    .await?;
    if let Some(name) = guard {
        Auth::session_guard(request, Some(&name))?.set_user(Box::new(user.clone()));
        Auth::guard(request, Some("sanctum"))?.set_user(Box::new(user.clone()));
        if enabling
            && previous_secret
                != crate::two_factor_authenticatable::text(&user, "two_factor_secret")
                    .map_err(internal)?
        {
            request
                .session_mut()
                .map_err(internal)?
                .put(
                    "auth.user_factor_setup",
                    json!(client::security(&user, &fortify)?),
                )
                .map_err(internal)?;
        } else if ending {
            request
                .session_mut()
                .map_err(internal)?
                .forget(&["auth.user_factor_setup"]);
        }
    }
    Ok((user, fortify, payload))
}
fn web_mutate<'a, M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &'a mut Request,
    mutation: Mutation<'a>,
) -> HttpFuture<'a, Result<(), HttpError>> {
    Box::pin(async move {
        change::<M>(request, Operation::from_native(mutation)?, true).await?;
        Ok(())
    })
}
async fn read<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    kind: Read,
    web: bool,
) -> Result<Value, HttpError> {
    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
    enabled(&fortify)?;
    let mut user = if web {
        super::web::user::<M>(request, &fortify).await?
    } else {
        account::user::<M>(request).await?
    };
    let actor = if web {
        None
    } else {
        account::actor(request, &user, Some("account:update"))?
    };
    let proof = if web {
        confirmation::claim_session(request, &user, &fortify).await?
    } else {
        confirmation::claim(request, &user, Purpose::Factors, &fortify).await?
    };
    let provider = App::make::<dyn contracts::TwoFactorAuthenticationProvider>().await?;
    let sanctum = App::make::<Sanctum>().await?;
    let database = App::make::<DatabaseManager>().await?;
    credentials::with_locked(&mut user, move |locked, tx| {
        Box::pin(async move {
            proof.validate_in_transaction(locked, &fortify, tx).await?;
            account::check_actor(
                locked,
                tx,
                &sanctum,
                &database,
                actor.clone(),
                Some("account:update"),
                &[],
            )
            .await?;
            let result = data(locked, &fortify, provider.as_ref(), kind)?;
            account::check_actor(
                locked,
                tx,
                &sanctum,
                &database,
                actor,
                Some("account:update"),
                &[],
            )
            .await?;
            proof.validate_in_transaction(locked, &fortify, tx).await?;
            Ok(result)
        })
    })
    .await
    .map_err(Into::into)
}
fn web_read<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    kind: Read,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Ok(client::no_store(Response::json(
            StatusCode::OK,
            ApiResult::success(read::<M>(request, kind, true).await?),
        )?))
    })
}
pub(super) fn api_enable<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let force = request.boolean("force").await?;
        let (_, _, payload) = change::<M>(request, Operation::Enable(force), false).await?;
        Ok(client::no_store(Response::json(
            StatusCode::OK,
            ApiResult::success(payload),
        )?))
    })
}
pub(super) fn api_confirm<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let code = request
            .input("code")
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let (user, fortify, _) = change::<M>(request, Operation::Confirm(code), false).await?;
        Ok(client::no_store(Response::json(
            StatusCode::OK,
            ApiResult::success(overview(&user, &fortify)?),
        )?))
    })
}
pub(super) fn api_disable<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        change::<M>(request, Operation::Disable, false).await?;
        Ok(client::no_store(Response::json(
            StatusCode::OK,
            ApiResult::empty(),
        )?))
    })
}
pub(super) fn api_generate<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let (_, _, payload) = change::<M>(request, Operation::Generate, false).await?;
        Ok(client::no_store(Response::json(
            StatusCode::OK,
            ApiResult::success(payload),
        )?))
    })
}
macro_rules! reader {
    ($name:ident,$kind:ident) => {
        pub(super) fn $name<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
            request: &mut Request,
        ) -> HttpFuture<'_, Result<Response, HttpError>> {
            Box::pin(async move {
                Ok(client::no_store(
                    Response::json(
                        StatusCode::OK,
                        ApiResult::success(read::<M>(request, Read::$kind, false).await?),
                    )?,
                ))
            })
        }
    };
}
reader!(api_qr, Qr);
reader!(api_secret, Secret);
reader!(api_codes, Codes);

pub(super) fn api_state<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        enabled(&fortify)?;
        let user = account::user::<M>(request).await?;
        account::actor(request, &user, Some("account:read"))?;
        Ok(client::no_store(Response::json(
            StatusCode::OK,
            ApiResult::success(overview(&user, &fortify)?),
        )?))
    })
}

fn routes<M: HasApiTokens + TwoFactorAuthenticatable + Clone>() -> Result<(), RoutingError> {
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    if !fortify.truthy("fortify.views")
        || !Features::new(fortify.config.clone())
            .enabled(Features::two_factor_authentication())
            .map_err(|_| RoutingError::InvalidAction)?
    {
        return Ok(());
    }
    Route::group(
        RouteGroup::new()
            .middleware_name("web")
            .middleware_name("sensitive")
            .middleware_name("auth.session")
            .middleware_name(format!(
                "auth:{}",
                fortify.guard().as_deref().unwrap_or("web")
            )),
        |router| {
            router
                .get("/user/two-factor-authentication", page::<M>)?
                .name("user.two-factor.page");
            Ok(())
        },
    )
}
fn page<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        enabled(&fortify)?;
        let user = super::web::user::<M>(request, &fortify).await?;
        let mut factor = overview(&user, &fortify)?;
        factor["svg"] = Value::Null;
        factor["secretKey"] = Value::Null;
        factor["recovery_codes"] = json!([]);
        if request
            .query(Some("setup"))?
            .as_ref()
            .and_then(Value::as_str)
            == Some("0")
        {
            request
                .session_mut()
                .map_err(internal)?
                .forget(&["auth.user_factor_setup"]);
        }
        let binding = client::security(&user, &fortify)?;
        let requested = request
            .session()
            .map_err(internal)?
            .get("auth.user_factor_setup")
            .and_then(Value::as_str)
            .is_some_and(|value| client::same_binding(value, &binding));
        let setup = factor["confirmation_required"] == true || requested;
        if setup && factor["has_secret"] == true {
            factor = read::<M>(request, Read::Setup, true).await?;
        } else if request
            .query(Some("recovery"))?
            .as_ref()
            .and_then(Value::as_str)
            == Some("1")
            && factor["has_secret"] == true
        {
            factor["recovery_codes"] =
                read::<M>(request, Read::Codes, true).await?["recovery_codes"].clone();
        }
        let view = View::make(
            "user.two-factor",
            [
                ("factor".into(), factor.into()),
                ("show_setup".into(), setup.into()),
            ]
            .into(),
        )?;
        Response::view(StatusCode::OK, view, Default::default(), request)
            .await
            .map(|response| {
                client::no_store(response).header(
                    "referrer-policy".parse().expect("静态头名称"),
                    "no-referrer".parse().expect("静态头值"),
                )
            })
    })
}
