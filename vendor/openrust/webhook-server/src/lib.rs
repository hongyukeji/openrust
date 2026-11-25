#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
mod backoff_strategy;
mod body;
mod call_webhook_job;
#[path = "../config/webhook_server.rs"]
mod configuration;
mod console;
mod delivery;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_deliveries_table.rs"]
mod delivery_migration;
mod delivery_store;
mod destination;
mod dispatch;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_endpoints_table.rs"]
mod endpoint_migration;
mod endpoints;
mod error;
pub mod events;
mod extensions;
mod recovery;
#[path = "../database/migrations/2026_11_25_000000_extend_webhook_deliveries_for_recovery.rs"]
mod recovery_migration;
mod replay;
#[path = "../database/migrations/2026_11_25_000000_extend_webhook_deliveries_for_replay.rs"]
mod replay_migration;
mod signer;
mod snapshot;
mod webhook_call;
mod worker;

pub use backoff_strategy::{BackoffStrategy, ExponentialBackoffStrategy};
pub use call_webhook_job::{CallWebhookJob, WebhookJobAttempt};
pub use delivery::{
    DeliveryContext, DeliveryPermission, DeliveryPolicy, DeliveryService, register_delivery_policy,
};
pub use delivery_store::{DeliveryProgress, DeliveryStore};
pub use destination::{BoundTarget, TargetPolicy};
pub use dispatch::{QueuedDelivery, SynchronousDelivery};
pub use endpoints::{Endpoint, EndpointState, EndpointStore};
pub use error::{Error, Result};
pub use extensions::{register_backoff_strategy, register_call_webhook_job, register_signer};
pub use recovery::RecoveryReport;
pub use replay::{
    ReplayDelivery, ReplayLimits, ReplayPhase, ReplayPolicy, ReplayRequest, register_replay_policy,
};
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

        static ENTRIES: &[MigrationEntry] = &[
            MigrationEntry::new(
                "vendor/openrust/webhook-server/database/migrations/2026_11_25_000000_create_webhook_deliveries_table.rs",
                &delivery_migration::MIGRATION,
            ),
            MigrationEntry::new(
                "vendor/openrust/webhook-server/database/migrations/2026_11_25_000000_create_webhook_endpoints_table.rs",
                &endpoint_migration::MIGRATION,
            ),
            MigrationEntry::new(
                "vendor/openrust/webhook-server/database/migrations/2026_11_25_000000_extend_webhook_deliveries_for_recovery.rs",
                &recovery_migration::MIGRATION,
            ),
            MigrationEntry::new(
                "vendor/openrust/webhook-server/database/migrations/2026_11_25_000000_extend_webhook_deliveries_for_replay.rs",
                &replay_migration::MIGRATION,
            ),
        ];
        ENTRIES
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(WebhookServerServiceProvider)]
}
