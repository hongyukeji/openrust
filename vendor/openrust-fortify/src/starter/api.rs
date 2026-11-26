//! 无需先建立浏览器会话的注册／密码恢复适配，继续使用原生Action与密码代理。
use super::{UserResource, client};
use crate::{
    Fortify, contracts,
    http::controllers,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::events::Registered,
    foundation::http::ValidatedFormRequest,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Facade as _,
};
use std::sync::Arc;

fn json_only(request: &mut Request) {
    request
        .parts_mut()
        .headers
        .insert("accept", "application/json".parse().expect("静态头值"));
}

/// 注册只创建身份并派发原生Registered事件；访问令牌仍须显式完成登录及额外因素。
pub fn register(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        if fortify.truthy("fortify.lowercase_usernames") {
            crate::actions::CanonicalizeUsername::new(fortify.clone())
                .handle(request)
                .await?;
        }
        let input = request.all().await?;
        let creator = fortify
            .creator
            .read()
            .expect("Fortify actions lock")
            .clone();
        let creator = match creator {
            Some(action) => action,
            None => {
                Resolver::<dyn contracts::CreatesNewUsers>::get_facade_root().map_err(internal)?
            }
        };
        let user = creator.create(&input).await?;
        let resource = UserResource::for_request(user.as_ref(), request).await?;
        let snapshot: Arc<dyn openrust::contracts::auth::Authenticatable> =
            user.clone_user().into();
        crate::actions::emit(Registered { user: snapshot }, &[]).await?;
        Ok(client::no_store(Response::json(
            StatusCode::CREATED,
            openrust::http::ApiResult::success(resource),
        )?))
    })
}

pub struct PasswordResetLinkController;
impl PasswordResetLinkController {
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
        validated: ValidatedFormRequest<crate::http::requests::SendPasswordResetLinkRequest>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        json_only(request);
        Box::pin(async move {
            controllers::PasswordResetLinkController
                .store(request, validated)
                .await
        })
    }
}

pub fn reset(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    json_only(request);
    Box::pin(async move { controllers::NewPasswordController.store(request).await })
}

pub fn resend(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    json_only(request);
    Box::pin(async move {
        controllers::EmailVerificationNotificationController
            .store(request)
            .await
    })
}
