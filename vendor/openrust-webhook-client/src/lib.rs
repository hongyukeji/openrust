#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#[path = "../database/migrations/2026_11_25_000012_add_webhook_attachment_cleanup_lease.rs"]
mod attachment_cleanup_migration;
#[path = "../database/migrations/2026_11_25_000011_add_webhook_attachment_storage.rs"]
mod attachment_migration;
#[path = "../database/migrations/2026_11_25_000006_create_webhook_processing_attempts_table.rs"]
mod attempt_migration;
#[path = "../config/webhook_client.rs"]
mod configuration;
mod console;
mod contracts;
#[path = "../database/migrations/2026_11_25_000005_add_deferred_processing_to_webhook_calls_table.rs"]
mod deferred_migration;
#[path = "../database/migrations/2026_11_25_000007_add_deferred_route_to_webhook_calls_table.rs"]
mod deferred_route_migration;
#[path = "../database/migrations/2026_11_25_000013_create_webhook_dispatch_intents_table.rs"]
mod dispatch_intents_migration;
mod error;
#[path = "../database/migrations/2026_11_25_000008_add_expiry_to_webhook_event_receipts_table.rs"]
mod event_expiry_migration;
mod factories;
pub mod hints;
#[path = "../database/migrations/2026_11_25_000009_create_webhook_hints_table.rs"]
mod hints_migration;
pub mod http;
#[path = "../database/migrations/2026_11_25_000004_create_webhook_event_receipts_table.rs"]
mod identity_migration;
pub mod jobs;
#[path = "../database/migrations/2026_11_25_000014_add_processing_recovery_to_webhook_dispatch_intents.rs"]
mod processing_recovery_migration;
pub mod replay;
#[path = "../database/migrations/2026_11_25_000015_create_webhook_replays_table.rs"]
mod replay_migration;
mod retention;
mod routes;
pub use routes::{WebhookRouteOptions, WebhookRouter, WebhookRoutes};
pub mod events;
#[path = "../database/migrations/2026_11_25_000003_add_processing_lease_to_webhook_calls_table.rs"]
mod lease_migration;
#[path = "../database/migrations/2026_11_25_000001_create_webhook_calls_table.rs"]
mod migration;
pub mod models;
mod payload;
#[path = "../database/migrations/2026_11_25_000002_add_payload_to_webhook_calls_table.rs"]
mod payload_migration;
pub use payload_migration::AddPayloadToWebhookCallsTable;
mod processor;
mod signature_validator;
mod storage;
#[path = "../database/migrations/2026_11_25_000001_extend_webhook_calls_for_update_time.rs"]
mod update_time_migration;
pub mod verification;
#[path = "../database/migrations/2026_11_25_000010_create_webhook_verification_audits_table.rs"]
mod verification_migration;
mod webhook_config;

pub use migration::CreateWebhookCallsTable;
pub use processor::{VerifiedWebhook, WebhookJobDispatcher, WebhookProcessor, WebhookRuntime};
pub use storage::{
    AttachmentCleanupReport, DatabaseWebhookStore, DispatchBacklog, ProcessingAttempt,
    ProcessingAttemptsPage, ReceiptDiagnostics, ReceiptDispatchMode, ReceiptPersistence,
    ReceiptState, WebhookStore,
};

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
            factories::defaults(app.container())?;
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
            app.container().bind::<WebhookConfig, _>(|container| {
                let name = openrust::support::facades::Route::current_route_name()?;
                let config =
                    container.get::<dyn openrust::contracts::config::Repository>("config")?;
                let unique = routes::unique_token(config.as_ref()).map_err(|source| {
                    openrust::contracts::container::BindingResolutionError::FactoryFailed {
                        service: "webhook-client",
                        source: Box::new(source),
                    }
                })?;
                container
                    .make::<WebhookConfigRepository>()?
                    .get_config_for_route(name.as_deref(), unique)
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
            self.load_migrations_with_application_overrides(app, Self::migrations())
                .await?;
            self.publishes(app, MIGRATION_SOURCES, "webhook-client-migrations")?;
            self.commands(app, console::register);
            app.make::<openrust::database::eloquent::ModelPruners>()
                .await?
                .register(
                    retention::MODEL,
                    "vendor/openrust-webhook-client/src/models.rs",
                    &retention::PruneWebhookCalls,
                )?;
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
openrust::compiled_migration_assets!("webhook-client";
    "2026_11_25_000001_create_webhook_calls_table.rs" => migration,
    "2026_11_25_000001_extend_webhook_calls_for_update_time.rs" => update_time_migration,
    "2026_11_25_000002_add_payload_to_webhook_calls_table.rs" => payload_migration,
    "2026_11_25_000003_add_processing_lease_to_webhook_calls_table.rs" => lease_migration,
    "2026_11_25_000004_create_webhook_event_receipts_table.rs" => identity_migration,
    "2026_11_25_000005_add_deferred_processing_to_webhook_calls_table.rs" => deferred_migration,
    "2026_11_25_000006_create_webhook_processing_attempts_table.rs" => attempt_migration,
    "2026_11_25_000007_add_deferred_route_to_webhook_calls_table.rs" => deferred_route_migration,
    "2026_11_25_000008_add_expiry_to_webhook_event_receipts_table.rs" => event_expiry_migration,
    "2026_11_25_000009_create_webhook_hints_table.rs" => hints_migration,
    "2026_11_25_000010_create_webhook_verification_audits_table.rs" => verification_migration,
    "2026_11_25_000011_add_webhook_attachment_storage.rs" => attachment_migration,
    "2026_11_25_000012_add_webhook_attachment_cleanup_lease.rs" => attachment_cleanup_migration,
    "2026_11_25_000013_create_webhook_dispatch_intents_table.rs" => dispatch_intents_migration,
    "2026_11_25_000014_add_processing_recovery_to_webhook_dispatch_intents.rs" => processing_recovery_migration,
    "2026_11_25_000015_create_webhook_replays_table.rs" => replay_migration,
);

impl WebhookClientServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        MIGRATION_ENTRIES
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(WebhookClientServiceProvider)]
}
