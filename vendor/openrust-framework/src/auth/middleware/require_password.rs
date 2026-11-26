use crate::{
    auth::{Auth, AuthError},
    contracts::{http::Middleware, session::Session as _},
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    session::{PASSWORD_CONFIRMED_AT, confirmation_seconds},
    support::facades::{Redirect, Url},
};

/// Requires a recent confirmation of the authenticated user's password.
#[derive(Default)]
pub struct RequirePassword {
    route: Option<String>,
    timeout: Option<i64>,
}

impl RequirePassword {
    pub fn new(route: Option<&str>, timeout: Option<i64>) -> Self {
        Self {
            route: route.filter(|route| !route.is_empty()).map(str::to_owned),
            timeout,
        }
    }

    /// 返回原生确认挑战；已有近期确认时返回None，供组合处理器复用同一跳转规则。
    pub fn challenge(&self, request: &mut Request) -> Result<Option<Response>, HttpError> {
        let confirmed = request
            .session()?
            .get(PASSWORD_CONFIRMED_AT)
            .and_then(serde_json::Value::as_i64);
        let timeout = self.timeout()?;
        if confirmed.is_some_and(|time| confirmation_seconds().saturating_sub(time) <= timeout) {
            return Ok(None);
        }
        if request.expects_json() {
            return Response::json(
                StatusCode::LOCKED,
                serde_json::json!({"message": "Password confirmation required."}),
            )
            .map(Some);
        }
        let target = Url::route(
            self.route.as_deref().unwrap_or("password.confirm"),
            &[],
            true,
        )?;
        Ok(Some(
            Redirect::guest(request, &target, Default::default())?.into(),
        ))
    }

    pub(crate) fn parse(parameters: &str) -> Option<Self> {
        let mut parts = parameters.split(',');
        let route = parts.next();
        let timeout = match parts.next() {
            None | Some("") => None,
            Some(seconds) => Some(seconds.parse().ok()?),
        };
        parts.next().is_none().then(|| Self::new(route, timeout))
    }

    fn timeout(&self) -> Result<i64, AuthError> {
        if let Some(timeout) = self.timeout {
            return Ok(timeout);
        }
        match Auth::manager()?.config().get("auth.password_timeout") {
            None | Some(serde_json::Value::Null) => Ok(10_800),
            Some(value) => value
                .as_i64()
                .map(|seconds| if seconds == 0 { 10_800 } else { seconds })
                .ok_or(AuthError::Configuration("authentication password timeout")),
        }
    }
}

impl Middleware for RequirePassword {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if let Some(response) = self.challenge(request)? {
                return Ok(response);
            }
            Ok(next.run(request).await)
        })
    }
}
