use crate::{
    Fortify, contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::Auth,
    contracts::auth::Guard as _,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{Facade as _, Redirect},
};

pub struct EmailVerificationPromptController;
pub struct EmailVerificationNotificationController;
pub struct VerifyEmailController;

pub(crate) fn intended(request: &mut Request) -> Result<Response, HttpError> {
    let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
    Ok(Redirect::intended(
        request,
        &fortify.redirects("email-verification", None),
        Default::default(),
    )?
    .into())
}
impl EmailVerificationPromptController {
    pub fn show<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request).await?.ok_or(HttpError::NotFound)?;
            let user = user.as_email_verification().ok_or_else(|| {
                internal(std::io::Error::other("User must implement MustVerifyEmail"))
            })?;
            if user.hasVerifiedEmail() {
                intended(request)
            } else {
                super::respond::<dyn contracts::VerifyEmailViewResponse>(request).await
            }
        })
    }
}
impl EmailVerificationNotificationController {
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request).await?.ok_or(HttpError::NotFound)?;
            let user = user.as_email_verification().ok_or_else(|| {
                internal(std::io::Error::other("User must implement MustVerifyEmail"))
            })?;
            if user.hasVerifiedEmail() {
                return if request.wantsJson() {
                    Ok(Response::noContent())
                } else {
                    intended(request)
                };
            }
            user.sendEmailVerificationNotification().await?;
            super::respond::<dyn contracts::EmailVerificationNotificationSentResponse>(request)
                .await
        })
    }
}
impl VerifyEmailController {
    pub fn verify<'a>(
        &'a self,
        request: &'a mut Request,
        _: openrust::foundation::http::ValidatedFormRequest<
            crate::http::requests::VerifyEmailRequest,
        >,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request).await?.ok_or(HttpError::NotFound)?;
            let mut account = crate::email_verification_model::account(user.as_ref())?;
            account.verify().await?;
            let fortify = Resolver::<Fortify>::getFacadeRoot().map_err(internal)?;
            Auth::sessionGuard(request, fortify.guard().as_deref())?.set_user(account.into_user());
            super::respond::<dyn contracts::VerifyEmailResponse>(request).await
        })
    }
}
