//! 客户端认证路径保持不可缓存，敏感凭据只接受请求正文或正式认证标头。
use openrust::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

pub(super) struct NoStore;
impl Middleware for NoStore {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            request
                .parts_mut()
                .headers
                .insert("accept", "application/json".parse().expect("静态头值"));
            let response = next.run(request).await;
            Ok(super::client::no_store(response).header(
                "pragma".parse().expect("静态头名称"),
                "no-cache".parse().expect("静态头值"),
            ))
        })
    }
}

pub(super) struct BodyCredentials;
impl Middleware for BodyCredentials {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !matches!(
                *request.method(),
                openrust::http::Method::GET | openrust::http::Method::HEAD
            ) {
                use openrust::support::facades::Facade as _;
                let fortify = crate::resolver::Resolver::<crate::Fortify>::get_facade_root()
                    .map_err(crate::resolver::internal)?;
                let query = request.query(None)?;
                if query.as_ref().is_some_and(|value| {
                    value.get(fortify.username()).is_some() || value.get(fortify.email()).is_some()
                }) {
                    return Err(HttpError::InvalidRequest);
                }
                if query.as_ref().is_some_and(|value| {
                    [
                        "password",
                        "password_confirmation",
                        "current_password",
                        "token",
                        "challenge",
                        "proof",
                        "code",
                        "recovery_code",
                        "credential",
                        "access_token",
                        "reauthentication_token",
                    ]
                    .iter()
                    .any(|field| value.get(*field).is_some())
                }) {
                    return Err(HttpError::InvalidRequest);
                }
            }
            Ok(next.run(request).await)
        })
    }
}

/// Sanctum可认证多态身份；套件账户操作只接受其显式注册的用户模型。
pub(super) struct UserModel<M>(std::marker::PhantomData<fn() -> M>);
impl<M> UserModel<M> {
    pub const fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}
impl<M: openrust::contracts::auth::Authenticatable> Middleware for UserModel<M> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = openrust::auth::Auth::user(request)
                .await?
                .ok_or(HttpError::Unauthenticated)?;
            if !(user.as_ref() as &dyn std::any::Any).is::<M>() {
                return Err(HttpError::Unauthenticated);
            }
            Ok(next.run(request).await)
        })
    }
}
