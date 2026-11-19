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

pub struct TwoFactorLoginResponse;
impl contracts::TwoFactorLoginResponse for TwoFactorLoginResponse {}
impl contracts::Response for TwoFactorLoginResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.wantsJson() {
                return Ok(Response::noContent());
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
pub struct FailedTwoFactorLoginResponse;
impl contracts::FailedTwoFactorLoginResponse for FailedTwoFactorLoginResponse {}
impl contracts::Response for FailedTwoFactorLoginResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let (field, message) = if request.filled(&["recovery_code"]).await? {
                (
                    "recovery_code",
                    "The provided two factor recovery code was invalid.",
                )
            } else {
                (
                    "code",
                    "The provided two factor authentication code was invalid.",
                )
            };
            let error = invalid(
                field,
                Lang::get(message, &Default::default(), None).map_err(internal)?,
            );
            if request.wantsJson() {
                return Err(HttpError::Validation(Box::new(error)));
            }
            Ok(
                Redirect::route("two-factor.login", &[], Default::default())?
                    .withErrors(request, error.errors(), "default")?
                    .into(),
            )
        })
    }
}
macro_rules! management_response {
    ($name:ident,$status:expr) => {
        pub struct $name;
        impl contracts::$name for $name {}
        impl contracts::Response for $name {
            fn toResponse<'a>(
                &'a self,
                request: &'a mut Request,
            ) -> HttpFuture<'a, Result<Response, HttpError>> {
                Box::pin(async move {
                    if request.wantsJson() {
                        return Response::json(StatusCode::OK, "");
                    }
                    Ok(Redirect::back(request, None, Default::default())?
                        .with(request, "status", $status.into())?
                        .into())
                })
            }
        }
    };
}
management_response!(
    TwoFactorEnabledResponse,
    Fortify::TWO_FACTOR_AUTHENTICATION_ENABLED
);
management_response!(
    TwoFactorConfirmedResponse,
    Fortify::TWO_FACTOR_AUTHENTICATION_CONFIRMED
);
management_response!(
    TwoFactorDisabledResponse,
    Fortify::TWO_FACTOR_AUTHENTICATION_DISABLED
);
management_response!(
    RecoveryCodesGeneratedResponse,
    Fortify::RECOVERY_CODES_GENERATED
);
pub(crate) struct TwoFactorChallengeViewResponse;
impl contracts::TwoFactorChallengeViewResponse for TwoFactorChallengeViewResponse {}
impl contracts::TwoFactorChallengeViewResponse for SimpleViewResponse {}
impl contracts::Response for TwoFactorChallengeViewResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            let view = fortify.two_factor_view().ok_or_else(|| {
                internal(std::io::Error::other(
                    "Fortify two-factor challenge view has not been configured",
                ))
            })?;
            contracts::Response::toResponse(view.as_ref(), request).await
        })
    }
}

pub struct RegisterResponse;
impl contracts::RegisterResponse for RegisterResponse {}
impl contracts::Response for RegisterResponse {
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
                &fortify.redirects("register", None),
                Default::default(),
            )?
            .into())
        })
    }
}
management_response!(
    ProfileInformationUpdatedResponse,
    Fortify::PROFILE_INFORMATION_UPDATED
);
management_response!(PasswordUpdateResponse, Fortify::PASSWORD_UPDATED);
pub(crate) struct RegisterViewResponse;
impl contracts::RegisterViewResponse for RegisterViewResponse {}
impl contracts::RegisterViewResponse for SimpleViewResponse {}
impl contracts::Response for RegisterViewResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            let view = fortify.register_view().ok_or_else(|| {
                internal(std::io::Error::other(
                    "Fortify register view has not been configured",
                ))
            })?;
            contracts::Response::toResponse(view.as_ref(), request).await
        })
    }
}

impl contracts::VerifyEmailViewResponse for SimpleViewResponse {}
pub(crate) struct VerifyEmailViewResponse;
impl contracts::VerifyEmailViewResponse for VerifyEmailViewResponse {}
impl contracts::Response for VerifyEmailViewResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            fortify
                .verify_email_view()
                .ok_or_else(|| {
                    internal(std::io::Error::other(
                        "Fortify email verification view has not been configured",
                    ))
                })?
                .toResponse(request)
                .await
        })
    }
}
pub struct VerifyEmailResponse;
impl contracts::VerifyEmailResponse for VerifyEmailResponse {}
impl contracts::Response for VerifyEmailResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.wantsJson() {
                return Ok(Response::noContent());
            }
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            Ok(Redirect::intended(
                request,
                &format!(
                    "{}?verified=1",
                    fortify.redirects("email-verification", None)
                ),
                Default::default(),
            )?
            .into())
        })
    }
}
pub struct EmailVerificationNotificationSentResponse;
impl contracts::EmailVerificationNotificationSentResponse
    for EmailVerificationNotificationSentResponse
{
}
impl contracts::Response for EmailVerificationNotificationSentResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.wantsJson() {
                return Response::json(StatusCode::ACCEPTED, "");
            }
            Ok(Redirect::back(request, None, Default::default())?
                .with(
                    request,
                    "status",
                    serde_json::json!(Fortify::VERIFICATION_LINK_SENT),
                )?
                .into())
        })
    }
}
