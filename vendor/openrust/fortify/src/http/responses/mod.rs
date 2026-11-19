use crate::{
    Fortify, LoginRateLimiter, contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{
        MessageBag,
        facades::{Facade as _, Lang, Redirect},
    },
    validation::ValidationException,
};
use std::collections::BTreeMap;

pub struct LoginResponse;
pub struct LogoutResponse;
pub struct PasswordConfirmedResponse;
pub struct FailedPasswordConfirmationResponse;
pub struct LockoutResponse;
impl contracts::LoginResponse for LoginResponse {}
impl contracts::LogoutResponse for LogoutResponse {}
impl contracts::PasswordConfirmedResponse for PasswordConfirmedResponse {}
impl contracts::FailedPasswordConfirmationResponse for FailedPasswordConfirmationResponse {}
impl contracts::LockoutResponse for LockoutResponse {}

pub(crate) fn invalid(field: &str, message: String) -> ValidationException {
    let mut errors = MessageBag::default();
    errors.add(field, message);
    ValidationException::new(errors)
}
impl contracts::Response for LoginResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.wantsJson() {
                return Response::json(StatusCode::OK, serde_json::json!({"two_factor":false}));
            }
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            Ok(Redirect::intended(
                request,
                &fortify.redirects("login", None),
                Default::default(),
            )?
            .into())
        })
    }
}
impl contracts::Response for LogoutResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.wantsJson() {
                return Ok(Response::noContent());
            }
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            Ok(Redirect::to(&fortify.redirects("logout", Some("/")), Default::default())?.into())
        })
    }
}
impl contracts::Response for PasswordConfirmedResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.wantsJson() {
                return Response::json(StatusCode::CREATED, "");
            }
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            Ok(Redirect::intended(
                request,
                &fortify.redirects("password-confirmation", None),
                Default::default(),
            )?
            .into())
        })
    }
}
impl contracts::Response for FailedPasswordConfirmationResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let message = Lang::get(
                "The provided password was incorrect.",
                &Default::default(),
                None,
            )
            .map_err(internal)?;
            let error = invalid("password", message);
            if request.wantsJson() {
                return Err(HttpError::Validation(Box::new(error)));
            }
            Ok(Redirect::back(request, None, Default::default())?
                .withErrors(request, error.errors(), "default")?
                .into())
        })
    }
}
impl contracts::Response for LockoutResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            let seconds = Resolver::<LoginRateLimiter>::getFacadeRoot()
                .map_err(internal)?
                .availableIn(request)
                .await?;
            let args = BTreeMap::from([
                ("seconds".into(), seconds.to_string()),
                ("minutes".into(), seconds.div_ceil(60).to_string()),
            ]);
            let message = Lang::get("auth.throttle", &args, None).map_err(internal)?;
            let mut error = invalid(&fortify.username(), message);
            error.status(StatusCode::TOO_MANY_REQUESTS);
            Err(HttpError::Validation(Box::new(error)))
        })
    }
}

/// A named native Blade view or an asynchronous application response callback.
#[derive(Clone)]
pub enum SimpleViewResponse {
    View(String),
    Callback(std::sync::Arc<ViewCallback>),
}
pub type ViewCallback =
    dyn for<'a> Fn(&'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> + Send + Sync;
impl From<String> for SimpleViewResponse {
    fn from(view: String) -> Self {
        Self::View(view)
    }
}
impl From<&str> for SimpleViewResponse {
    fn from(view: &str) -> Self {
        Self::View(view.into())
    }
}
impl SimpleViewResponse {
    pub fn callback(callback: std::sync::Arc<ViewCallback>) -> Self {
        Self::Callback(callback)
    }
}
impl contracts::LoginViewResponse for SimpleViewResponse {}
impl contracts::ConfirmPasswordViewResponse for SimpleViewResponse {}
impl contracts::Response for SimpleViewResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            match self {
                Self::Callback(callback) => callback(request).await,
                Self::View(name) => {
                    let view =
                        openrust::support::facades::View::make(name.clone(), Default::default())
                            .map_err(internal)?;
                    Response::view(StatusCode::OK, view, Default::default(), request).await
                }
            }
        })
    }
}
pub(crate) struct LoginViewResponse;
pub(crate) struct ConfirmPasswordViewResponse;
impl contracts::LoginViewResponse for LoginViewResponse {}
impl contracts::ConfirmPasswordViewResponse for ConfirmPasswordViewResponse {}
async fn configured_view(request: &mut Request, login: bool) -> Result<Response, HttpError> {
    let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
    let view = fortify.view(login).ok_or_else(|| {
        internal(std::io::Error::other(
            "Fortify view has not been configured",
        ))
    })?;
    contracts::Response::toResponse(view.as_ref(), request).await
}
impl contracts::Response for LoginViewResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(configured_view(request, true))
    }
}
impl contracts::Response for ConfirmPasswordViewResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(configured_view(request, false))
    }
}
