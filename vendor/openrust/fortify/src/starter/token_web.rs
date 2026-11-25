//! 个人令牌网页复用账户事务；新令牌只在当前响应展示，不进入闪存或URL。
use super::{account, client};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    sanctum::HasApiTokens,
    support::facades::{Facade as _, Redirect, Route, View},
};

pub(super) fn routes<M: HasApiTokens + TwoFactorAuthenticatable + Clone>()
-> Result<(), RoutingError> {
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    if !fortify.truthy("fortify.views") {
        return Ok(());
    }
    let group = RouteGroup::new()
        .middleware_name("web")
        .middleware_name("auth.session")
        .middleware_name(format!(
            "auth:{}",
            fortify.guard().as_deref().unwrap_or("web")
        ));
    Route::group(group, |router| {
        router
            .get("/user/api-tokens", index::<M>)?
            .name("user.api-tokens.index");
        router
            .post("/user/api-tokens", store::<M>)?
            .name("user.api-tokens.store");
        router
            .delete("/user/api-tokens/{token}", destroy::<M>)?
            .name("user.api-tokens.destroy");
        Ok(())
    })
}
fn index<M: HasApiTokens + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move { render::<M>(request, None, StatusCode::OK).await })
}
fn store<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let issued = account::issue_token::<M>(request, true).await?;
        if request.expects_json() {
            return Ok(client::no_store(
                openrust::http::ApiResponse::success(issued)
                    .with_status(StatusCode::CREATED)
                    .into_response()?,
            ));
        }
        render::<M>(
            request,
            issued["token"].as_str().map(str::to_owned),
            StatusCode::CREATED,
        )
        .await
    })
}
async fn render<M: HasApiTokens + Clone>(
    request: &mut Request,
    new_token: Option<String>,
    status: StatusCode,
) -> Result<Response, HttpError> {
    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
    let user = account::user::<M>(request).await?;
    let abilities = client::available_abilities(&user, &fortify)?;
    let tokens = account::token_summaries::<M>(request, true).await?;
    let view = View::make(
        "user.api-tokens",
        [
            ("tokens".into(), tokens.into()),
            (
                "available_abilities".into(),
                serde_json::json!(abilities).into(),
            ),
            (
                "new_token".into(),
                serde_json::to_value(new_token).map_err(internal)?.into(),
            ),
        ]
        .into(),
    )?;
    let response =
        client::no_store(Response::view(status, view, Default::default(), request).await?);
    Ok(response.header(
        "referrer-policy".parse().expect("静态头名称"),
        "no-referrer".parse().expect("静态头值"),
    ))
}
fn destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        account::revoke_token_for::<M>(request, true).await?;
        if request.expects_json() {
            return Ok(client::no_store(Response::empty(StatusCode::NO_CONTENT)));
        }
        Ok(client::no_store(
            Redirect::route("user.api-tokens.index", &[], Default::default())?
                .with(request, "status", "user.token_revoked".into())?
                .into(),
        ))
    })
}
