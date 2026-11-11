use crate::{
    auth::{Auth, AuthError},
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    support::facades::URL,
};

/// Requires verified email for users that declare the verification capability.
#[derive(Default)]
pub struct EnsureEmailIsVerified {
    route: Option<String>,
}

impl EnsureEmailIsVerified {
    pub fn redirect_to(route: impl Into<String>) -> Self {
        Self {
            route: Some(route.into()),
        }
    }
}

impl Middleware for EnsureEmailIsVerified {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request).await?;
            if user.as_ref().is_some_and(|user| {
                user.as_email_verification()
                    .is_none_or(|user| user.hasVerifiedEmail())
            }) {
                return Ok(next.run(request).await);
            }
            if request.expectsJson() {
                return Err(HttpError::AuthorizationDenied(
                    "Your email address is not verified.".into(),
                ));
            }
            let url = URL::route(
                self.route.as_deref().unwrap_or("verification.notice"),
                &[],
                true,
            )?;
            let location = url
                .parse::<::http::HeaderValue>()
                .map_err(AuthError::backend)?;
            Ok(Response::empty(StatusCode::FOUND).header(::http::header::LOCATION, location))
        })
    }
}
