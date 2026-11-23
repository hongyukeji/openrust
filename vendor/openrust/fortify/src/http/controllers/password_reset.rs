use crate::{
    Fortify,
    actions::CompletePasswordReset,
    contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{
        Auth, AuthError, Credentials,
        passwords::{Password, PasswordResetStatus},
    },
    contracts::validation::Validator as _,
    foundation::http::ValidatedFormRequest,
    http::{HttpError, HttpFuture, Request, Response},
    support::{
        Str,
        facades::{Facade as _, Validator},
    },
    validation::{Data, RuleSet},
};
use std::sync::{Arc, Mutex};

pub struct PasswordResetLinkController;
pub struct NewPasswordController;

async fn canonicalize(request: &mut Request, fortify: &Fortify) -> Result<(), HttpError> {
    let email = fortify.email();
    if fortify.truthy("fortify.lowercase_usernames")
        && request.has(&[&email]).await?
        && let Some(value) = request.input(Some(&email)).await?
    {
        request
            .merge(serde_json::json!({email:Str::lower(&scalar_string(value)?)}))
            .await?;
    }
    Ok(())
}
fn scalar_string(value: serde_json::Value) -> Result<String, HttpError> {
    Ok(match value {
        serde_json::Value::String(value) => value,
        serde_json::Value::Null | serde_json::Value::Bool(false) => String::new(),
        serde_json::Value::Bool(true) => "1".into(),
        serde_json::Value::Number(value) => {
            if value.is_f64() {
                openrust::support::php_float_string(value.as_f64().unwrap())
            } else {
                value.to_string()
            }
        }
        _ => {
            return Err(internal(std::io::Error::other(
                "Password reset value cannot be coerced to a string",
            )));
        }
    })
}
async fn credentials(request: &mut Request, fortify: &Fortify) -> Result<Credentials, HttpError> {
    let email = fortify.email();
    let value = request
        .input(Some(&email))
        .await?
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    Ok(Credentials::new(
        [(email, openrust::database::Value::Text(value))],
        "",
    ))
}
fn broker_name(fortify: &Fortify) -> Option<String> {
    fortify
        .config
        .get("fortify.passwords")
        .and_then(|v| v.as_str().map(str::to_owned))
}
async fn respond<T: contracts::PasswordResponse + ?Sized + 'static>(
    request: &mut Request,
    status: PasswordResetStatus,
) -> Result<Response, HttpError> {
    Resolver::<T>::get_facade_root()
        .map_err(internal)?
        .toResponse(request, status)
        .await
}
impl PasswordResetLinkController {
    pub fn create<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(super::respond::<
            dyn contracts::RequestPasswordResetLinkViewResponse,
        >(request))
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
        _: ValidatedFormRequest<super::super::requests::SendPasswordResetLinkRequest>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            canonicalize(request, &fortify).await?;
            let status = Password::broker(broker_name(&fortify).as_deref())?
                .sendResetLink(&credentials(request, &fortify).await?, None)
                .await?;
            if status == PasswordResetStatus::ResetLinkSent {
                respond::<dyn contracts::SuccessfulPasswordResetLinkRequestResponse>(
                    request, status,
                )
                .await
            } else {
                respond::<dyn contracts::FailedPasswordResetLinkRequestResponse>(request, status)
                    .await
            }
        })
    }
}
impl NewPasswordController {
    pub fn create<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(super::respond::<dyn contracts::ResetPasswordViewResponse>(
            request,
        ))
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            Auth::sessionGuard(request, fortify.guard().as_deref())?;
            canonicalize(request, &fortify).await?;
            Validator::make(
                Data::fromRequest(request).await?,
                RuleSet::parse([
                    ("token", "required"),
                    (fortify.email().as_str(), "required|email"),
                    ("password", "required"),
                ])?,
            )?
            .validate()
            .await?;
            let input = Arc::new(request.all().await?);
            let token = scalar_string(request.input(Some("token")).await?.unwrap_or_default())?;
            let password = request
                .input(Some("password"))
                .await?
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let completion =
                Resolver::<CompletePasswordReset>::get_facade_root().map_err(internal)?;
            let errors = Arc::new(Mutex::new(None));
            let action_fortify = fortify.clone();
            let callback_errors = errors.clone();
            let callback: Box<openrust::contracts::auth::ResetCallback<'_>> =
                Box::new(move |user, _| {
                    let input = input.clone();
                    let fortify = action_fortify.clone();
                    let errors = callback_errors.clone();
                    let completion = completion.clone();
                    Box::pin(async move {
                        let result = async {
                            let action = fortify
                                .password_resetter
                                .read()
                                .expect("Fortify actions lock")
                                .clone();
                            let action = match action {
                                Some(action) => action,
                                None => {
                                    Resolver::<dyn contracts::ResetsUserPasswords>::get_facade_root(
                                    )
                                    .map_err(internal)?
                                }
                            };
                            action.reset(user, input.as_ref()).await?;
                            completion.handle(user).await
                        }
                        .await;
                        match result {
                            Ok(()) => Ok(()),
                            Err(error) => {
                                *errors.lock().expect("Password reset error lock") = Some(error);
                                Err(AuthError::Configuration(
                                    "Fortify password reset action failed",
                                ))
                            }
                        }
                    })
                });
            let status = Password::broker(broker_name(&fortify).as_deref())?
                .reset(
                    &credentials(request, &fortify).await?,
                    &token,
                    password.as_bytes(),
                    callback.as_ref(),
                )
                .await;
            if let Some(error) = errors.lock().expect("Password reset error lock").take() {
                return Err(error);
            }
            let status = status?;
            if status == PasswordResetStatus::PasswordReset {
                respond::<dyn contracts::PasswordResetResponse>(request, status).await
            } else {
                respond::<dyn contracts::FailedPasswordResetResponse>(request, status).await
            }
        })
    }
}
