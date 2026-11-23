use crate::{
    Fortify, FortifyError, contracts,
    resolver::{Resolver, internal},
    two_factor_model::{Account, Mutation, present, text, truthy},
};
use openrust::{
    auth::Auth,
    contracts::{auth::Guard as _, session::Session as _},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Facade as _,
};
use zeroize::Zeroizing;

async fn account(request: &mut Request, fortify: &Fortify) -> Result<Box<dyn Account>, HttpError> {
    let user = Auth::user(request).await?.ok_or(HttpError::NotFound)?;
    fortify
        .account(user.as_ref())
        .ok_or_else(|| internal(FortifyError::UnregisteredTwoFactorModel))
}
async fn mutate(request: &mut Request, mutation: Mutation<'_>) -> Result<(), HttpError> {
    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
    let mut user = account(request, &fortify).await?;
    let enabling = matches!(mutation, Mutation::Enable(_));
    user.mutate(mutation).await?;
    if enabling
        && fortify
            .confirmsTwoFactorAuthentication()
            .map_err(internal)?
        && present(user.attribute("two_factor_secret").map_err(internal)?)
        && !present(
            user.attribute("two_factor_confirmed_at")
                .map_err(internal)?,
        )
    {
        request
            .session_mut()
            .map_err(internal)?
            .forget(&["two_factor_confirming_at"]);
    }
    Auth::sessionGuard(request, fortify.guard().as_deref())?.set_user(user.into_user());
    Ok(())
}
pub struct TwoFactorAuthenticationController;
impl TwoFactorAuthenticationController {
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let force = request.boolean("force", false, false).await?;
            mutate(request, Mutation::Enable(force)).await?;
            super::respond::<dyn contracts::TwoFactorEnabledResponse>(request).await
        })
    }
    pub fn destroy<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            mutate(request, Mutation::Disable).await?;
            super::respond::<dyn contracts::TwoFactorDisabledResponse>(request).await
        })
    }
}
pub struct ConfirmedTwoFactorAuthenticationController;
impl ConfirmedTwoFactorAuthenticationController {
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let code = super::two_factor_authenticated_session_controller::input(request, "code")
                .await?
                .unwrap_or_default();
            mutate(request, Mutation::Confirm(&code)).await?;
            super::respond::<dyn contracts::TwoFactorConfirmedResponse>(request).await
        })
    }
}
pub struct RecoveryCodeController;
impl RecoveryCodeController {
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            mutate(request, Mutation::Generate).await?;
            super::respond::<dyn contracts::RecoveryCodesGeneratedResponse>(request).await
        })
    }
    pub fn index<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let user = account(request, &fortify).await?;
            if !truthy(user.attribute("two_factor_secret").map_err(internal)?)
                || !truthy(
                    user.attribute("two_factor_recovery_codes")
                        .map_err(internal)?,
                )
            {
                return Response::json(StatusCode::OK, serde_json::json!([]));
            }
            // This endpoint returns arbitrary decoded PHP JSON; the trait's code list is stricter.
            let encrypted = text(
                user.attribute("two_factor_recovery_codes")
                    .map_err(internal)?,
            )
            .map_err(internal)?
            .ok_or_else(|| internal(FortifyError::InvalidAttribute("two_factor_recovery_codes")))?;
            let clear = Zeroizing::new(
                fortify
                    .currentEncrypter()
                    .map_err(internal)?
                    .decrypt(&encrypted, true)
                    .map_err(internal)?,
            );
            let codes: serde_json::Value =
                serde_json::from_slice(&clear).unwrap_or(serde_json::Value::Null);
            Response::json(StatusCode::OK, codes)
        })
    }
}
pub struct TwoFactorSecretKeyController;
impl TwoFactorSecretKeyController {
    pub fn show<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let user = account(request, &fortify).await?;
            let encrypted = text(user.attribute("two_factor_secret").map_err(internal)?)
                .map_err(internal)?
                .ok_or_else(|| {
                    openrust::http::abort(
                        StatusCode::NOT_FOUND,
                        "Two factor authentication has not been enabled.",
                        Default::default(),
                    )
                })?;
            let clear = Zeroizing::new(
                fortify
                    .currentEncrypter()
                    .map_err(internal)?
                    .decrypt(&encrypted, true)
                    .map_err(internal)?,
            );
            let secret = std::str::from_utf8(&clear).map_err(internal)?;
            Response::json(StatusCode::OK, serde_json::json!({"secretKey":secret}))
        })
    }
}
pub struct TwoFactorQrCodeController;
impl TwoFactorQrCodeController {
    pub fn show<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let user = account(request, &fortify).await?;
            if !present(user.attribute("two_factor_secret").map_err(internal)?) {
                return Response::json(StatusCode::OK, serde_json::json!([]));
            }
            let provider =
                Resolver::<dyn contracts::TwoFactorAuthenticationProvider>::get_facade_root()
                    .map_err(internal)?;
            let svg = user.qr_svg(&fortify, provider.as_ref()).map_err(internal)?;
            let url = user.qr_url(&fortify, provider.as_ref()).map_err(internal)?;
            Response::json(StatusCode::OK, serde_json::json!({"svg":svg,"url":url}))
        })
    }
}
