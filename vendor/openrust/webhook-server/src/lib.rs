#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
mod backoff_strategy;
#[path = "../config/webhook_server.rs"]
mod configuration;
mod error;
mod signer;

pub use backoff_strategy::{BackoffStrategy, ExponentialBackoffStrategy};
pub use error::{Error, Result};
pub use signer::{DefaultSigner, Signer, SigningContext, StandardWebhookSigner};

use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

pub struct WebhookServerServiceProvider;
impl ServiceProvider for WebhookServerServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, configuration::load(&env)?, "webhook-server")?;
            }
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[(
                    "config/webhook_server.rs",
                    include_bytes!("../config/webhook_server.rs"),
                )],
                "webhook-server-config",
            )?;
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(WebhookServerServiceProvider)]
}
