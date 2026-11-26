use crate::{
    auth::AuthResult,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

use super::authenticate::{authenticate, redirect, redirect_target};

/// Redirects users who are already authenticated.
pub struct RedirectIfAuthenticated {
    guards: Vec<String>,
    location: ::http::HeaderValue,
}

impl Default for RedirectIfAuthenticated {
    fn default() -> Self {
        Self {
            guards: Vec::new(),
            location: ::http::HeaderValue::from_static("/"),
        }
    }
}

impl RedirectIfAuthenticated {
    pub fn new(
        guards: impl IntoIterator<Item = impl Into<String>>,
        path: &str,
    ) -> AuthResult<Self> {
        Ok(Self {
            guards: guards.into_iter().map(Into::into).collect(),
            location: redirect_target(path)?,
        })
    }
}

impl Middleware for RedirectIfAuthenticated {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if authenticate(request, &self.guards).await? {
                return Ok(redirect(self.location.clone()));
            }
            Ok(next.run(request).await)
        })
    }
}
