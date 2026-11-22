#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use crate::{ApiTokenCookieFactory, Passport};
use openrust::{
    contracts::{container::Container as _, http::Middleware, session::Session as _},
    http::{HttpError, HttpFuture, Method, Next, Request, Response},
    support::facades::{Auth, Cookie},
};
/// Add to the web group after Session and cookie middleware.
#[derive(Default)]
pub struct CreateFreshApiToken {
    guard: Option<String>,
}
impl CreateFreshApiToken {
    pub fn using(guard: Option<&str>) -> Self {
        Self {
            guard: guard.map(str::to_owned),
        }
    }
}
impl Middleware for CreateFreshApiToken {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let passport = openrust::container::Container::getInstance().make::<Passport>()?;
            let name = passport.cookie(None);
            let initially_authenticated = if request.hasCookie(&name)? {
                Auth::guard(request, self.guard.as_deref())?.check().await?
            } else {
                false
            };
            let response = next.run(request).await;
            let user = Auth::guard(request, self.guard.as_deref())?
                .user()
                .await?
                .map(|u| u.getAuthIdentifier());
            if initially_authenticated && user.is_none() {
                return response.withCookie(&Cookie::forget(&name, None, None)?, request);
            }
            if request.method() != Method::GET || user.is_none() {
                return Ok(response);
            }
            if response
                .headers()
                .get_all(axum::http::header::SET_COOKIE)
                .iter()
                .filter_map(|h| h.to_str().ok())
                .any(|h| h.split_once('=').is_some_and(|(n, _)| n == name))
            {
                return Ok(response);
            }
            let csrf = request
                .session()?
                .token()
                .ok_or(HttpError::InvalidRequest)?;
            let cookie = ApiTokenCookieFactory::make(user.expect("checked user"), csrf).map_err(
                |source| HttpError::Internal {
                    source: Box::new(source),
                },
            )?;
            response.withCookie(&cookie, request)
        })
    }
}
