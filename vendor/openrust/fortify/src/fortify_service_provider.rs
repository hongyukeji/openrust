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
            for feature in [
                Features::registration(),
                Features::resetPasswords(),
                Features::emailVerification(),
                Features::updateProfileInformation(),
                Features::updatePasswords(),
                Features::twoFactorAuthentication(),
                Features::passkeys(),
            ] {
                if features.enabled(feature)? {
                    return Err(std::io::Error::other(format!(
                        "Fortify feature {feature} HTTP integration is not implemented"
                    ))
                    .into());
                }
            }
            self.loadRoutesFrom(app, crate::http::routes::register)?;
            Ok(())
        })
    }
}
