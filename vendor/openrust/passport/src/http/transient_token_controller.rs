#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use super::controllers::{internal, select_guard};
use crate::ApiTokenCookieFactory;
use openrust::{
    contracts::session::Session as _,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Auth,
};
pub struct TransientTokenController;
impl TransientTokenController {
    pub fn refresh(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            select_guard(request)?;
            let user = Auth::user(request)
                .await?
                .ok_or(HttpError::Unauthenticated)?;
            let csrf = request
                .session()?
                .token()
                .ok_or(HttpError::InvalidRequest)?;
            let cookie =
                ApiTokenCookieFactory::make(user.get_auth_identifier(), csrf).map_err(internal)?;
            Response::new(StatusCode::OK, Body::from("Refreshed.")).with_cookie(&cookie, request)
        })
    }
}
