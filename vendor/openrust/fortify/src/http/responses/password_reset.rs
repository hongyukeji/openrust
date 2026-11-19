use super::*;
use openrust::auth::passwords::PasswordResetStatus;

macro_rules! password_response {
    ($name:ident, $success:expr, $reset:expr) => {
        pub struct $name;
        impl contracts::$name for $name {}
        impl contracts::PasswordResponse for $name {
            fn toResponse<'a>(
                &'a self,
                request: &'a mut Request,
                status: PasswordResetStatus,
            ) -> HttpFuture<'a, Result<Response, HttpError>> {
                Box::pin(render(request, status, $success, $reset))
            }
        }
    };
}
password_response!(PasswordResetResponse, true, true);
password_response!(FailedPasswordResetResponse, false, true);
password_response!(SuccessfulPasswordResetLinkRequestResponse, true, false);
password_response!(FailedPasswordResetLinkRequestResponse, false, false);

async fn render(
    request: &mut Request,
    status: PasswordResetStatus,
    success: bool,
    reset: bool,
) -> Result<Response, HttpError> {
    let message = Lang::get(status.as_str(), &Default::default(), None).map_err(internal)?;
    if !success {
        // The fixed upstream responses use literal email even with fortify.email configured.
        let error = invalid("email", message);
        if request.wantsJson() {
            return Err(HttpError::Validation(Box::new(error)));
        }
        return Ok(Redirect::back(request, None, Default::default())?
            .onlyInput(request, &["email"])
            .await?
            .withErrors(request, error.errors(), "default")?
            .into());
    }
    if request.wantsJson() {
        return Response::json(StatusCode::OK, serde_json::json!({"message":message}));
    }
    let redirect = if reset {
        let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
        let login = if fortify.truthy("fortify.views") {
            Some(openrust::support::facades::URL::route("login", &[], true)?)
        } else {
            None
        };
        Redirect::to(
            &fortify.redirects("password-reset", login.as_deref()),
            Default::default(),
        )?
    } else {
        Redirect::back(request, None, Default::default())?
    };
    Ok(redirect.with(request, "status", message.into())?.into())
}

macro_rules! password_view {
    ($name:ident, $getter:ident) => {
        pub(crate) struct $name;
        impl contracts::$name for $name {}
        impl contracts::$name for SimpleViewResponse {}
        impl contracts::Response for $name {
            fn toResponse<'a>(
                &'a self,
                request: &'a mut Request,
            ) -> HttpFuture<'a, Result<Response, HttpError>> {
                Box::pin(async move {
                    let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
                    fortify
                        .$getter()
                        .ok_or_else(|| {
                            internal(std::io::Error::other(
                                "Fortify password reset view has not been configured",
                            ))
                        })?
                        .toResponse(request)
                        .await
                })
            }
        }
    };
}
password_view!(
    RequestPasswordResetLinkViewResponse,
    request_password_reset_link_view
);
password_view!(ResetPasswordViewResponse, reset_password_view);
