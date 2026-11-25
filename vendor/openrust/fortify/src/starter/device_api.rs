//! 本人浏览器设备接口；身份、能力及近期证明复用账户中心的原生接线。
use super::{
    account, client,
    confirmation::{self, Purpose},
    credentials, devices,
};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    contracts::session::Session as _,
    database::DatabaseManager,
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, StatusCode},
    sanctum::{HasApiTokens, Sanctum},
    session::SessionManager,
    support::facades::{App, Auth, Facade as _},
};

fn selected() -> Result<std::sync::Arc<devices::Policy>, HttpError> {
    devices::policy()?.ok_or(HttpError::NotFound)
}

pub(super) fn index<M: HasApiTokens + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let policy = selected()?;
        let user = account::user::<M>(request).await?;
        let actor = account::actor(request, &user, Some("account:read"))?;
        let current = if actor.is_none() {
            Some(request.session().map_err(internal)?.get_id().to_owned())
        } else {
            None
        };
        let manager = App::make::<SessionManager>().await?;
        let devices = policy.list(&user, &manager, current.as_deref()).await?;
        Ok(client::no_store(
            ApiResponse::success(devices).into_response()?,
        ))
    })
}

pub(super) fn destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    revoke::<M>(request, false, false)
}
pub(super) fn destroy_others<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    revoke::<M>(request, true, false)
}
fn revoke<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    others: bool,
    browser: bool,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let devices = selected()?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = account::user::<M>(request).await?;
        let actor = if browser {
            None
        } else {
            account::actor(request, &user, Some("account:update"))?
        };
        let current = if actor.is_none() {
            Some(request.session().map_err(internal)?.get_id().to_owned())
        } else {
            None
        };
        let target = if others {
            None
        } else {
            Some(
                request
                    .route("session")
                    .ok_or(HttpError::NotFound)?
                    .to_owned(),
            )
        };
        let drop_current = match (&target, &current) {
            (Some(target), Some(raw)) => devices.is_current(&user, raw, target)?,
            _ => false,
        };
        let proof = if browser {
            confirmation::claim_session(request, &user, &fortify)?
        } else {
            confirmation::claim(request, &user, Purpose::Devices, &fortify).await?
        };
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        let guard = devices.guard.clone();
        credentials::with_locked(&mut user, move |locked, transaction| {
            Box::pin(async move {
                proof
                    .validate_in_transaction(locked, &fortify, transaction)
                    .await?;
                account::check_actor(
                    locked,
                    transaction,
                    &sanctum,
                    &database,
                    actor,
                    Some("account:update"),
                    &[],
                )
                .await?;
                if others {
                    devices
                        .revoke_others(locked, transaction, current.as_deref())
                        .await?;
                } else if !devices
                    .revoke(
                        locked,
                        transaction,
                        target.as_deref().expect("单设备操作已取得目标"),
                    )
                    .await?
                {
                    return Err(HttpError::NotFound.into());
                }
                // 原生remember token为用户共享。清除旧恢复凭据，但不改变其他有效会话的密码指纹。
                locked.set_remember_token(None);
                if !locked.save_in_transaction(transaction).await? {
                    return Err(openrust::auth::AuthError::Configuration(
                        "用户模型取消设备凭据变更",
                    ));
                }
                locked.refresh_for_update(transaction).await?;
                proof.validate(locked, &fortify)
            })
        })
        .await?;
        if drop_current {
            Auth::session_guard(request, Some(&guard))?
                .logout_current_device()
                .await?;
        }
        Ok(client::no_store(Response::empty(StatusCode::NO_CONTENT)))
    })
}

pub(super) fn revoke_browser<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    others: bool,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    revoke::<M>(request, others, true)
}
