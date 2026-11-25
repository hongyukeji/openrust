#![doc = include_str!("../README.md")]

#[path = "../config/translate.rs"]
mod configuration;
mod contract;
mod error;
mod facade;
mod manager;

pub use contract::{Translation, TranslationFuture, TranslationRequest, Translator};
pub use error::{TranslationError, TranslationResult};
pub use facade::Translate;
pub use manager::TranslateManager;

use std::sync::Arc;

use openrust::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

pub struct TranslateServiceProvider;

impl ServiceProvider for TranslateServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, configuration::load(&env)?, "translate")?;
            }
            let container = app.container();
            container.singleton_named("translator", |container| {
                Ok(Arc::new(TranslateManager::new(container.get("config")?)))
            })?;
            container.alias("translator", ServiceKey::typed::<TranslateManager>())?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[(
                    "config/translate.rs",
                    include_bytes!("../config/translate.rs"),
                )],
                "translate-config",
            )
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(TranslateServiceProvider)]
}
