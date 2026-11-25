#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
mod backoff_strategy;
mod body;
mod call_webhook_job;
#[path = "../config/webhook_server.rs"]
mod configuration;
mod console;
mod delivery;
#[path = "../database/migrations/2026_11_25_000000_mark_webhook_deliveries_retention_hold.rs"]
mod delivery_hold_migration;
#[path = "../database/migrations/2026_11_25_000000_mark_webhook_deliveries_idempotency_expiry.rs"]
mod delivery_idempotency_expiry_migration;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_deliveries_table.rs"]
mod delivery_migration;
mod delivery_store;
mod destination;
mod diagnostics;
mod dispatch;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_endpoints_table.rs"]
mod endpoint_migration;
mod endpoints;
mod error;
pub mod events;
mod extensions;
mod history_retention;
mod idempotency;
mod idempotency_expiry;
#[path = "../database/migrations/2026_11_25_000000_index_webhook_idempotency_record.rs"]
mod idempotency_index_migration;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_idempotency_keys_table.rs"]
mod idempotency_migration;
mod limits;
#[path = "../database/migrations/2026_11_25_000000_mark_webhook_messages_retention_hold.rs"]
mod message_hold_migration;
#[path = "../database/migrations/2026_11_25_000000_mark_webhook_messages_idempotency_expiry.rs"]
mod message_idempotency_expiry_migration;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_messages_table.rs"]
mod message_migration;
mod message_retention;
#[path = "../database/migrations/2026_11_25_000000_extend_webhook_messages_for_retention.rs"]
mod message_retention_migration;
mod proxy_map;
mod publication;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_rate_buckets_table.rs"]
mod rate_migration;
mod recovery;
#[path = "../database/migrations/2026_11_25_000000_extend_webhook_deliveries_for_recovery.rs"]
mod recovery_migration;
mod replay;
pub use proxy_map::ProxyMap;
#[path = "../database/migrations/2026_11_25_000000_extend_webhook_deliveries_for_replay.rs"]
mod replay_migration;
mod replay_retention;
mod retention;
mod retention_hold;
#[path = "../database/migrations/2026_11_25_000000_extend_webhook_deliveries_for_retention.rs"]
mod retention_migration;
mod signer;
mod snapshot;
#[path = "../database/migrations/2026_11_25_000000_create_webhook_subscriptions_table.rs"]
mod subscription_migration;
mod webhook_call;
mod worker;

pub use backoff_strategy::{BackoffStrategy, ExponentialBackoffStrategy};
pub use call_webhook_job::{CallWebhookJob, WebhookJobAttempt};
pub use delivery::{
    DeliveryContext, DeliveryPermission, DeliveryPolicy, DeliveryService, register_delivery_policy,
};
pub use delivery_store::{DeliveryProgress, DeliveryStore};
pub use destination::{BoundTarget, TargetPolicy};
pub use diagnostics::{
    DeliveryAttemptDiagnostics, DeliveryAttemptsPage, DeliveryDiagnostics, MessageDiagnostics,
};
pub use dispatch::{QueuedDelivery, SynchronousDelivery};
pub use endpoints::{Endpoint, EndpointState, EndpointStore};
pub use error::{Error, Result};
pub use extensions::{register_backoff_strategy, register_call_webhook_job, register_signer};
pub use history_retention::HistoryPruneReport;
pub use idempotency::IdempotencyKind;
pub use idempotency_expiry::IdempotencyPruneReport;
pub use limits::{DeliveryLimit, DeliveryLimits, register_delivery_limits};
pub use message_retention::PublicationPruneReport;
pub use publication::{EventType, PublishOptions, PublishedMessage};
pub use recovery::RecoveryReport;
pub use replay::{
    ReplayDelivery, ReplayLimits, ReplayPhase, ReplayPolicy, ReplayRequest, register_replay_policy,
};
pub use replay_retention::ReplayAuditPruneReport;
pub use retention::{DeliveryPruneReport, DeliveryRetention};
pub use retention_hold::RetentionRecord;
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
            self.load_migrations_with_application_overrides(app, Self::migrations())
                .await?;
            self.publishes(app, MIGRATION_SOURCES, "webhook-server-migrations")?;
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
openrust::compiled_migration_assets!("webhook-server";
    "2026_11_25_000000_create_webhook_deliveries_table.rs" => delivery_migration,
    "2026_11_25_000000_create_webhook_endpoints_table.rs" => endpoint_migration,
    "2026_11_25_000000_create_webhook_messages_table.rs" => message_migration,
    "2026_11_25_000000_create_webhook_rate_buckets_table.rs" => rate_migration,
    "2026_11_25_000000_create_webhook_subscriptions_table.rs" => subscription_migration,
    "2026_11_25_000000_extend_webhook_deliveries_for_recovery.rs" => recovery_migration,
    "2026_11_25_000000_extend_webhook_deliveries_for_replay.rs" => replay_migration,
    "2026_11_25_000000_extend_webhook_deliveries_for_retention.rs" => retention_migration,
    "2026_11_25_000000_extend_webhook_messages_for_retention.rs" => message_retention_migration,
    "2026_11_25_000000_mark_webhook_deliveries_retention_hold.rs" => delivery_hold_migration,
    "2026_11_25_000000_mark_webhook_messages_retention_hold.rs" => message_hold_migration,
    "2026_11_25_000000_create_webhook_idempotency_keys_table.rs" => idempotency_migration,
    "2026_11_25_000000_index_webhook_idempotency_record.rs" => idempotency_index_migration,
    "2026_11_25_000000_mark_webhook_deliveries_idempotency_expiry.rs" => delivery_idempotency_expiry_migration,
    "2026_11_25_000000_mark_webhook_messages_idempotency_expiry.rs" => message_idempotency_expiry_migration,
);

impl WebhookServerServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        MIGRATION_ENTRIES
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(WebhookServerServiceProvider)]
}
