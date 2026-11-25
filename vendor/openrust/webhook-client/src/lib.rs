#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#[path = "../config/webhook_client.rs"]
mod configuration;
mod contracts;
mod error;
mod signature_validator;
mod webhook_config;

pub use contracts::{
    DefaultRespondsTo, ProcessEverythingWebhookProfile, RespondsToWebhook, SignatureValidator,
    WebhookProfile, WebhookRequest,
};
pub use error::{Error, Result};
pub use signature_validator::{DefaultSignatureValidator, StandardSignatureValidator};
pub use webhook_config::{WebhookConfig, WebhookConfigRepository};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

pub struct WebhookClientServiceProvider;
impl ServiceProvider for WebhookClientServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, configuration::load(&env)?, "webhook-client")?;
            }
            app.container()
                .scoped::<WebhookConfigRepository, _>(|container| {
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    let env = container.make::<openrust::config::Environment>()?;
                    let value = config.get("webhook-client").ok_or_else(|| {
                        openrust::contracts::container::BindingResolutionError::FactoryFailed {
                            service: "webhook-client",
                            source: Box::new(Error::InvalidConfig("webhook-client")),
                        }
                    })?;
                    WebhookConfigRepository::from_configuration(&value, env)
                        .map(Arc::new)
                        .map_err(|source| {
                            openrust::contracts::container::BindingResolutionError::FactoryFailed {
                                service: "webhook-client",
                                source: Box::new(source),
                            }
                        })
                })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[(
                    "config/webhook_client.rs",
                    include_bytes!("../config/webhook_client.rs"),
                )],
                "webhook-client-config",
            )?;
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(WebhookClientServiceProvider)]
}
