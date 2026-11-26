use crate::{contracts, http::responses as native};
use openrust::http::{ApiResult, HttpError, HttpFuture, Request, Response, StatusCode};

fn private(response: Response) -> Response {
    response
        .header(
            "cache-control".parse().expect("静态头名称"),
            "no-store".parse().expect("静态头值"),
        )
        .header(
            "pragma".parse().expect("静态头名称"),
            "no-cache".parse().expect("静态头值"),
        )
}

macro_rules! account_response {
    ($name:ident, $status:expr) => {
        pub(super) struct $name;
        impl contracts::$name for $name {}
        impl contracts::Response for $name {
            fn to_response<'a>(
                &'a self,
                request: &'a mut Request,
            ) -> HttpFuture<'a, Result<Response, HttpError>> {
                Box::pin(async move {
                    if !request.wants_json() {
                        return contracts::Response::to_response(&native::$name, request).await;
                    }
                    let user = super::UserResource::current(request).await?;
                    Ok(private(Response::json($status, ApiResult::success(user))?))
                })
            }
        }
    };
}
account_response!(LoginResponse, StatusCode::OK);
account_response!(RegisterResponse, StatusCode::CREATED);
account_response!(TwoFactorLoginResponse, StatusCode::OK);

macro_rules! empty_response {
    ($($name:ident => $status:expr),+ $(,)?) => {$(
        pub(super) struct $name;
        impl contracts::$name for $name {}
        impl contracts::Response for $name {
            fn to_response<'a>(&'a self, request: &'a mut Request)
                -> HttpFuture<'a, Result<Response, HttpError>> {
                Box::pin(async move {
                    if !request.wants_json() {
                        return contracts::Response::to_response(&native::$name, request).await;
                    }
                    Ok(private(Response::json($status, ApiResult::empty())?))
                })
            }
        }
    )+};
}
empty_response!(
    ProfileInformationUpdatedResponse => StatusCode::OK,
    PasswordUpdateResponse => StatusCode::OK,
    PasswordConfirmedResponse => StatusCode::CREATED,
    EmailVerificationNotificationSentResponse => StatusCode::ACCEPTED,
    TwoFactorEnabledResponse => StatusCode::OK,
    TwoFactorConfirmedResponse => StatusCode::OK,
    TwoFactorDisabledResponse => StatusCode::OK,
    RecoveryCodesGeneratedResponse => StatusCode::OK,
);

macro_rules! password_response {
    ($($name:ident),+ $(,)?) => {$(
        pub(super) struct $name;
        impl contracts::$name for $name {}
        impl contracts::PasswordResponse for $name {
            fn to_response<'a>(&'a self, request: &'a mut Request,
                status: openrust::auth::passwords::PasswordResetStatus)
                -> HttpFuture<'a, Result<Response, HttpError>> {
                Box::pin(async move {
                    if !request.wants_json() {
                        return contracts::PasswordResponse::to_response(&native::$name, request, status).await;
                    }
                    let message = openrust::support::facades::Lang::get(status.as_str(), &Default::default(), None)
                        .map_err(crate::resolver::internal)?;
                    Ok(private(Response::json(
                        StatusCode::OK,
                        ApiResult::empty().with_message(message),
                    )?))
                })
            }
        }
    )+};
}
password_response!(
    PasswordResetResponse,
    SuccessfulPasswordResetLinkRequestResponse
);
