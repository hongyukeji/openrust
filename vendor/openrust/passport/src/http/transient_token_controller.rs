#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

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
                ApiTokenCookieFactory::make(user.getAuthIdentifier(), csrf).map_err(internal)?;
            Response::new(StatusCode::OK, Body::from("Refreshed.")).withCookie(&cookie, request)
        })
    }
}
