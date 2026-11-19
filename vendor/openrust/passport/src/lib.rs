//! Laravel Passport's application configuration, publishable schema and key command.
#![forbid(unsafe_code)]
#[path = "../config/passport.rs"]
mod configuration;
pub mod console;
mod keys;
pub mod migrations;
mod passport;

pub use keys::{CryptKey, KeyError, PrivateKey, PublicKey};
pub use passport::Passport;

use openrust::{
    config::Environment,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

/// Registers Passport configuration and the implemented publishing / key lifecycle.
pub struct PassportServiceProvider;
impl ServiceProvider for PassportServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let env = app.make::<Environment>().await?;
            self.mergeConfigFrom(app, configuration::load(&env)?, "passport")?;
            app.container().singleton::<Passport, _>(|container| {
                Ok(Arc::new(Passport::new(
                    container.get("config")?,
                    container.make::<openrust::foundation::Paths>()?,
                )))
            })?;
            app.container().singleton::<PrivateKey, _>(|container| {
                let passport = container.make::<Passport>()?;
                let key = passport.make_crypt_key("private").map_err(|error| {
                    openrust::contracts::container::BindingResolutionError::FactoryFailed {
                        service: "Passport cryptographic key",
                        source: Box::new(error),
                    }
                })?;
                Ok(Arc::new(PrivateKey(key)))
            })?;
            app.container().singleton::<PublicKey, _>(|container| {
                let passport = container.make::<Passport>()?;
                let key = passport.make_crypt_key("public").map_err(|error| {
                    openrust::contracts::container::BindingResolutionError::FactoryFailed {
                        service: "Passport cryptographic key",
                        source: Box::new(error),
                    }
                })?;
                Ok(Arc::new(PublicKey(key)))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            // Laravel publishes these migrations; it does not silently run package schema.
            self.publishesMigrations(app, migrations::SOURCES, "passport-migrations")?;
            self.publishes(
                app,
                &[(
                    "config/passport.rs",
                    include_bytes!("../config/passport.rs"),
                )],
                "passport-config",
            )?;
            self.commands(app, console::commands::register);
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(PassportServiceProvider)]
}
