use crate::{
    auth::{Auth, AuthError, AuthResult},
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
};

/// Requires authentication through one of the selected guards.
#[derive(Default)]
pub struct Authenticate {
    guards: Vec<String>,
    login: Option<::http::HeaderValue>,
}

impl Authenticate {
    pub fn new(guards: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            guards: guards.into_iter().map(Into::into).collect(),
            login: None,
        }
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's authentication redirect configuration"
    )]
    pub fn redirectTo(mut self, path: &str) -> AuthResult<Self> {
        self.login = Some(redirect_target(path)?);
        Ok(self)
    }
}

impl Middleware for Authenticate {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if authenticate(request, &self.guards).await? {
                return Ok(next.run(request).await);
            }
            if !request.expectsJson()
                && let Some(path) = &self.login
            {
                return Ok(redirect(path.clone()));
            }
            Err(HttpError::Unauthenticated)
        })
    }
}

pub(super) async fn authenticate(request: &mut Request, names: &[String]) -> AuthResult<bool> {
    if names.is_empty() {
        return Auth::guard(request, None)?.check().await;
    }
    for name in names {
        if Auth::guard(request, Some(name))?.check().await? {
            Auth::shouldUse(request, name)?;
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn redirect_target(path: &str) -> AuthResult<::http::HeaderValue> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\\')
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(AuthError::Configuration("authentication redirect"));
    }
    path.parse().map_err(AuthError::backend)
}

pub(super) fn redirect(path: ::http::HeaderValue) -> Response {
    Response::empty(StatusCode::FOUND).header(::http::header::LOCATION, path)
}
