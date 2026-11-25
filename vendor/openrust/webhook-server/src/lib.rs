#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
mod backoff_strategy;
mod body;
#[path = "../config/webhook_server.rs"]
mod configuration;
mod console;
mod delivery;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_deliveries_table.rs"]
mod delivery_migration;
mod delivery_store;
mod destination;
mod error;
mod extensions;
mod signer;
mod snapshot;
mod webhook_call;
mod worker;

pub use backoff_strategy::{BackoffStrategy, ExponentialBackoffStrategy};
pub use delivery::{
    DeliveryContext, DeliveryPermission, DeliveryPolicy, DeliveryService, register_delivery_policy,
};
pub use delivery_store::{DeliveryProgress, DeliveryStore};
pub use destination::{BoundTarget, TargetPolicy};
pub use error::{Error, Result};
pub use extensions::{register_backoff_strategy, register_signer};
pub use signer::{DefaultSigner, Signer, SigningContext, StandardWebhookSigner};
pub use webhook_call::{PreparedWebhook, WebhookCall};

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
            self.load_migrations_from(app, Self::migrations()).await?;
            self.commands(app, console::register);
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
impl WebhookServerServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        use openrust::database::migrations::MigrationEntry;

        static ENTRIES: &[MigrationEntry] = &[MigrationEntry::new(
            "vendor/openrust/webhook-server/database/migrations/2026_11_25_000000_create_webhook_deliveries_table.rs",
            &delivery_migration::MIGRATION,
        )];
        ENTRIES
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(WebhookServerServiceProvider)]
}
