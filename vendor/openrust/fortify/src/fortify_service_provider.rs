use crate::{
    Features, Fortify, LoginRateLimiter, TwoFactorAuthenticationProvider, configuration, contracts,
    http::{controllers::*, responses},
};
use openrust::{
    config::Environment,
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

pub struct FortifyServiceProvider;
fn binding(error: impl std::error::Error + Send + Sync + 'static) -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "Fortify",
        source: Box::new(error),
    }
}
impl ServiceProvider for FortifyServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app
                .container()
                .get::<dyn openrust::contracts::config::Repository>("config")?;
            let environment = app.make::<Environment>().await?;
            self.mergeConfigFrom(
                app,
                configuration::load(config.as_ref(), &environment)?,
                "fortify",
            )?;
            let container = app.container();
            container.singleton::<Fortify, _>(|container| {
                Ok(Arc::new(Fortify::new(container.get("config")?)))
            })?;
            container.singleton::<LoginRateLimiter, _>(|container| {
                Ok(Arc::new(LoginRateLimiter::new(
                    container.make::<openrust::cache::RateLimiter>()?,
                    container.make::<Fortify>()?,
                )))
            })?;
            container.singleton::<dyn contracts::TwoFactorAuthenticationProvider, _>(
                |container| {
                    let cache = container
                        .make::<openrust::cache::CacheManager>()?
                        .store(None)
                        .map_err(binding)?;
                    Ok(Arc::new(
                        TwoFactorAuthenticationProvider::new(Some(cache))
                            .withConfig(container.get("config")?),
                    ))
                },
            )?;
            container.bind::<dyn contracts::RedirectsIfTwoFactorAuthenticatable, _>(
                |container| {
                    Ok(Arc::new(
                        crate::actions::RedirectIfTwoFactorAuthenticatable::new(
                            container.make::<Fortify>()?,
                            container.make::<LoginRateLimiter>()?,
                        ),
                    ))
                },
            )?;
            container.bind::<crate::actions::EnableTwoFactorAuthentication, _>(|container| {
                Ok(Arc::new(
                    crate::actions::EnableTwoFactorAuthentication::new(
                        container.make::<dyn contracts::TwoFactorAuthenticationProvider>()?,
                        container.make::<Fortify>()?,
                    ),
                ))
            })?;
            container.bind::<crate::actions::ConfirmTwoFactorAuthentication, _>(|container| {
                Ok(Arc::new(
                    crate::actions::ConfirmTwoFactorAuthentication::new(
                        container.make::<dyn contracts::TwoFactorAuthenticationProvider>()?,
                        container.make::<Fortify>()?,
                    ),
                ))
            })?;
            container.bind::<crate::actions::DisableTwoFactorAuthentication, _>(|container| {
                Ok(Arc::new(
                    crate::actions::DisableTwoFactorAuthentication::new(
                        container.make::<Fortify>()?,
                    ),
                ))
            })?;
            container.bind::<crate::actions::GenerateNewRecoveryCodes, _>(|container| {
                Ok(Arc::new(crate::actions::GenerateNewRecoveryCodes::new(
                    container.make::<Fortify>()?,
                )))
            })?;
            container.singleton::<AuthenticatedSessionController, _>(|_| {
                Ok(Arc::new(AuthenticatedSessionController))
            })?;
            container.singleton::<ConfirmablePasswordController, _>(|_| {
                Ok(Arc::new(ConfirmablePasswordController))
            })?;
            container.singleton::<ConfirmedPasswordStatusController, _>(|_| {
                Ok(Arc::new(ConfirmedPasswordStatusController))
            })?;
            macro_rules! response {
                ($name:ident) => {
                    container
                        .singleton::<dyn contracts::$name, _>(|_| Ok(Arc::new(responses::$name)))?;
                };
            }
            container.singleton::<TwoFactorAuthenticatedSessionController, _>(|_| {
                Ok(Arc::new(TwoFactorAuthenticatedSessionController))
            })?;
            container.singleton::<TwoFactorAuthenticationController, _>(|_| {
                Ok(Arc::new(TwoFactorAuthenticationController))
            })?;
            container.singleton::<ConfirmedTwoFactorAuthenticationController, _>(|_| {
                Ok(Arc::new(ConfirmedTwoFactorAuthenticationController))
            })?;
            container.singleton::<TwoFactorQrCodeController, _>(|_| {
                Ok(Arc::new(TwoFactorQrCodeController))
            })?;
            container.singleton::<TwoFactorSecretKeyController, _>(|_| {
                Ok(Arc::new(TwoFactorSecretKeyController))
            })?;
            container
                .singleton::<RecoveryCodeController, _>(|_| Ok(Arc::new(RecoveryCodeController)))?;
            container.singleton::<RegisteredUserController, _>(|_| {
                Ok(Arc::new(RegisteredUserController))
            })?;
            container.singleton::<ProfileInformationController, _>(|_| {
                Ok(Arc::new(ProfileInformationController))
            })?;
            container.singleton::<PasswordController, _>(|_| Ok(Arc::new(PasswordController)))?;
            container.singleton::<EmailVerificationPromptController, _>(|_| {
                Ok(Arc::new(EmailVerificationPromptController))
            })?;
            container.singleton::<EmailVerificationNotificationController, _>(|_| {
                Ok(Arc::new(EmailVerificationNotificationController))
            })?;
            container
                .singleton::<VerifyEmailController, _>(|_| Ok(Arc::new(VerifyEmailController)))?;
            container.singleton::<PasswordResetLinkController, _>(|_| {
                Ok(Arc::new(PasswordResetLinkController))
            })?;
            container
                .singleton::<NewPasswordController, _>(|_| Ok(Arc::new(NewPasswordController)))?;
            container.bind::<crate::actions::CompletePasswordReset, _>(|container| {
                Ok(Arc::new(crate::actions::CompletePasswordReset::new(
                    container.make::<Fortify>()?,
                )))
            })?;
            response!(RequestPasswordResetLinkViewResponse);
            response!(ResetPasswordViewResponse);
            response!(PasswordResetResponse);
            response!(FailedPasswordResetResponse);
            response!(SuccessfulPasswordResetLinkRequestResponse);
            response!(FailedPasswordResetLinkRequestResponse);
            response!(VerifyEmailViewResponse);
            response!(VerifyEmailResponse);
            response!(EmailVerificationNotificationSentResponse);
            response!(RegisterResponse);
            response!(RegisterViewResponse);
            response!(ProfileInformationUpdatedResponse);
            response!(PasswordUpdateResponse);
            response!(TwoFactorLoginResponse);
            response!(FailedTwoFactorLoginResponse);
            response!(TwoFactorEnabledResponse);
            response!(TwoFactorConfirmedResponse);
            response!(TwoFactorDisabledResponse);
            response!(RecoveryCodesGeneratedResponse);
            response!(TwoFactorChallengeViewResponse);
            response!(LoginResponse);
            response!(LogoutResponse);
            response!(PasswordConfirmedResponse);
            response!(FailedPasswordConfirmationResponse);
            response!(LockoutResponse);
            response!(LoginViewResponse);
            response!(ConfirmPasswordViewResponse);
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let fortify = app.make::<Fortify>().await?;
            if !fortify.registersRoutes() {
                return Ok(());
            }
            let features = Features::new(fortify.config.clone());
            if features.enabled(Features::passkeys())? {
                return Err(std::io::Error::other(format!(
                    "Fortify feature {} HTTP integration is not implemented",
                    Features::passkeys()
                ))
                .into());
            }
            self.loadRoutesFrom(app, crate::http::routes::register)?;
            Ok(())
        })
    }
}
