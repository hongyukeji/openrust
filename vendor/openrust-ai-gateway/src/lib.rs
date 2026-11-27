//! 可独立启用的 AI 协议网关。
#![forbid(unsafe_code)]

use std::sync::Arc;

use openrust::{
    config::Environment,
    contracts::{container::Container as _, foundation::Application as _},
    database::{DatabaseManager, migrations::MigrationEntry},
    foundation::Application,
    http::HttpFuture,
    log::LogManager,
    support::{ProviderResult, ServiceProvider},
};

#[path = "../database/migrations/2026_11_26_000010_add_ai_gateway_account_budgets.rs"]
mod account_budget_migration;
mod accounts;
mod aggregate_costs;
#[path = "../database/migrations/2026_11_26_000011_create_ai_gateway_cost_totals_table.rs"]
mod aggregate_costs_migration;
mod billing;
#[path = "../database/migrations/2026_11_26_000013_create_ai_gateway_charge_adjustments_table.rs"]
mod billing_adjustments_migration;
#[path = "../database/migrations/2026_11_26_000006_create_ai_gateway_charges_table.rs"]
mod billing_migration;
mod budget;
#[path = "../database/migrations/2026_11_26_000009_create_ai_gateway_budgets_table.rs"]
mod budget_migration;
#[path = "../database/migrations/2026_11_26_000014_create_ai_gateway_budget_settlements_table.rs"]
mod budget_settlements_migration;
mod codex_catalog;
mod commands;
mod compatibility;
#[path = "../config/ai_gateway.rs"]
mod configuration;
#[path = "../database/migrations/2026_11_26_000012_create_ai_gateway_cost_pages_tables.rs"]
mod cost_pages_migration;
mod gateway;
#[path = "../database/migrations/2026_11_27_000019_add_expiry_to_ai_gateway_gemini_batch_files.rs"]
mod gemini_batch_file_expiry_migration;
#[path = "../database/migrations/2026_11_27_000020_add_parts_to_ai_gateway_gemini_batch_files.rs"]
mod gemini_batch_file_parts_migration;
#[path = "../database/migrations/2026_11_27_000018_create_ai_gateway_gemini_batch_files_table.rs"]
mod gemini_batch_files_migration;
mod gemini_batch_input;
#[path = "../database/migrations/2026_11_26_000017_create_ai_gateway_gemini_batches_table.rs"]
mod gemini_batches_migration;
mod invocation;
#[path = "../database/migrations/2026_11_26_000008_add_invocation_to_ai_gateway_usage.rs"]
mod invocation_migration;
#[path = "../database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs"]
mod key_migration;
mod localization;
mod managed_keys;
#[path = "../database/migrations/2026_11_26_000016_add_output_images_to_ai_gateway_usage.rs"]
mod output_images_migration;
mod output_quota;
#[path = "../database/migrations/2026_11_26_000003_create_ai_gateway_output_quota_table.rs"]
mod output_quota_migration;
mod protocol;
mod reconciliation;
#[path = "../database/migrations/2026_11_26_000007_create_ai_gateway_cost_evidence_tables.rs"]
mod reconciliation_migration;
#[path = "../database/migrations/2026_11_26_000015_add_reported_cost_to_ai_gateway_charges.rs"]
mod reported_cost_migration;
mod session_affinity;
mod usage;
#[path = "../database/migrations/2026_11_26_000002_create_ai_gateway_usage_table.rs"]
mod usage_migration;
#[path = "../database/migrations/2026_11_26_000005_add_observation_to_ai_gateway_usage.rs"]
mod usage_observation_migration;
#[path = "../database/migrations/2026_11_26_000004_add_key_scope_to_ai_gateway_usage.rs"]
mod usage_scope_migration;

pub use configuration::load as default_config;
use gateway::Gateway;
use managed_keys::KeyStore;
use serde::Deserialize;
use usage::UsageStore;

const KEY_MIGRATION: &str = "database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs";
static KEY_MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs",
    &key_migration::MIGRATION,
)];

const USAGE_MIGRATION: &str =
    "database/migrations/2026_11_26_000002_create_ai_gateway_usage_table.rs";
const OUTPUT_QUOTA_MIGRATION: &str =
    "database/migrations/2026_11_26_000003_create_ai_gateway_output_quota_table.rs";
const USAGE_SCOPE_MIGRATION: &str =
    "database/migrations/2026_11_26_000004_add_key_scope_to_ai_gateway_usage.rs";
const USAGE_OBSERVATION_MIGRATION: &str =
    "database/migrations/2026_11_26_000005_add_observation_to_ai_gateway_usage.rs";
const BILLING_MIGRATION: &str =
    "database/migrations/2026_11_26_000006_create_ai_gateway_charges_table.rs";
const RECONCILIATION_MIGRATION: &str =
    "database/migrations/2026_11_26_000007_create_ai_gateway_cost_evidence_tables.rs";
const INVOCATION_MIGRATION: &str =
    "database/migrations/2026_11_26_000008_add_invocation_to_ai_gateway_usage.rs";
const BUDGET_MIGRATION: &str =
    "database/migrations/2026_11_26_000009_create_ai_gateway_budgets_table.rs";
const ACCOUNT_BUDGET_MIGRATION: &str =
    "database/migrations/2026_11_26_000010_add_ai_gateway_account_budgets.rs";
const AGGREGATE_COSTS_MIGRATION: &str =
    "database/migrations/2026_11_26_000011_create_ai_gateway_cost_totals_table.rs";
const COST_PAGES_MIGRATION: &str =
    "database/migrations/2026_11_26_000012_create_ai_gateway_cost_pages_tables.rs";
const BILLING_ADJUSTMENTS_MIGRATION: &str =
    "database/migrations/2026_11_26_000013_create_ai_gateway_charge_adjustments_table.rs";
const BUDGET_SETTLEMENTS_MIGRATION: &str =
    "database/migrations/2026_11_26_000014_create_ai_gateway_budget_settlements_table.rs";
static USAGE_MIGRATIONS: &[MigrationEntry] = &[
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000002_create_ai_gateway_usage_table.rs",
        &usage_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000003_create_ai_gateway_output_quota_table.rs",
        &output_quota_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000004_add_key_scope_to_ai_gateway_usage.rs",
        &usage_scope_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000005_add_observation_to_ai_gateway_usage.rs",
        &usage_observation_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000006_create_ai_gateway_charges_table.rs",
        &billing_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000007_create_ai_gateway_cost_evidence_tables.rs",
        &reconciliation_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000008_add_invocation_to_ai_gateway_usage.rs",
        &invocation_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000009_create_ai_gateway_budgets_table.rs",
        &budget_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000010_add_ai_gateway_account_budgets.rs",
        &account_budget_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000011_create_ai_gateway_cost_totals_table.rs",
        &aggregate_costs_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000012_create_ai_gateway_cost_pages_tables.rs",
        &cost_pages_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000013_create_ai_gateway_charge_adjustments_table.rs",
        &billing_adjustments_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000014_create_ai_gateway_budget_settlements_table.rs",
        &budget_settlements_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000015_add_reported_cost_to_ai_gateway_charges.rs",
        &reported_cost_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000016_add_output_images_to_ai_gateway_usage.rs",
        &output_images_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_26_000017_create_ai_gateway_gemini_batches_table.rs",
        &gemini_batches_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_27_000018_create_ai_gateway_gemini_batch_files_table.rs",
        &gemini_batch_files_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_27_000019_add_expiry_to_ai_gateway_gemini_batch_files.rs",
        &gemini_batch_file_expiry_migration::MIGRATION,
    ),
    MigrationEntry::new(
        "vendor/openrust-ai-gateway/database/migrations/2026_11_27_000020_add_parts_to_ai_gateway_gemini_batch_files.rs",
        &gemini_batch_file_parts_migration::MIGRATION,
    ),
];

#[derive(Default, Deserialize)]
struct DatabaseSettings {
    #[serde(default)]
    enabled: bool,
    connection: Option<String>,
}

pub struct AiGatewayServiceProvider;

impl ServiceProvider for AiGatewayServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() && app.config()?.get("ai_gateway").is_none() {
                let env = app.make::<Environment>().await?;
                self.merge_config_from(app, default_config(&env)?, "ai_gateway")?;
            }
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = &[(
                "config/ai_gateway.rs",
                include_bytes!("../config/ai_gateway.rs").as_slice(),
            )];
            self.publishes(app, config, "ai-gateway-config")?;
            self.publishes_migrations(
                app,
                &[
                    (
                        KEY_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs"),
                    ),
                    (
                        USAGE_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000002_create_ai_gateway_usage_table.rs"),
                    ),
                    (
                        OUTPUT_QUOTA_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000003_create_ai_gateway_output_quota_table.rs"),
                    ),
                    (
                        USAGE_SCOPE_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000004_add_key_scope_to_ai_gateway_usage.rs"),
                    ),
                    (
                        USAGE_OBSERVATION_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000005_add_observation_to_ai_gateway_usage.rs"),
                    ),
                    (
                        BILLING_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000006_create_ai_gateway_charges_table.rs"),
                    ),
                    (
                        RECONCILIATION_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000007_create_ai_gateway_cost_evidence_tables.rs"),
                    ),
                    (
                        INVOCATION_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000008_add_invocation_to_ai_gateway_usage.rs"),
                    ),
                    (
                        BUDGET_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000009_create_ai_gateway_budgets_table.rs"),
                    ),
                    (
                        ACCOUNT_BUDGET_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000010_add_ai_gateway_account_budgets.rs"),
                    ),
                    (
                        AGGREGATE_COSTS_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000011_create_ai_gateway_cost_totals_table.rs"),
                    ),
                    (
                        COST_PAGES_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000012_create_ai_gateway_cost_pages_tables.rs"),
                    ),
                    (
                        BILLING_ADJUSTMENTS_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000013_create_ai_gateway_charge_adjustments_table.rs"),
                    ),
                    (
                        BUDGET_SETTLEMENTS_MIGRATION,
                        include_bytes!("../database/migrations/2026_11_26_000014_create_ai_gateway_budget_settlements_table.rs"),
                    ),
                    (
                        "database/migrations/2026_11_26_000015_add_reported_cost_to_ai_gateway_charges.rs",
                        include_bytes!("../database/migrations/2026_11_26_000015_add_reported_cost_to_ai_gateway_charges.rs"),
                    ),
                    (
                        "database/migrations/2026_11_26_000016_add_output_images_to_ai_gateway_usage.rs",
                        include_bytes!("../database/migrations/2026_11_26_000016_add_output_images_to_ai_gateway_usage.rs"),
                    ),
                    (
                        "database/migrations/2026_11_26_000017_create_ai_gateway_gemini_batches_table.rs",
                        include_bytes!("../database/migrations/2026_11_26_000017_create_ai_gateway_gemini_batches_table.rs"),
                    ),
                    (
                        "database/migrations/2026_11_27_000018_create_ai_gateway_gemini_batch_files_table.rs",
                        include_bytes!("../database/migrations/2026_11_27_000018_create_ai_gateway_gemini_batch_files_table.rs"),
                    ),
                    (
                        "database/migrations/2026_11_27_000019_add_expiry_to_ai_gateway_gemini_batch_files.rs",
                        include_bytes!("../database/migrations/2026_11_27_000019_add_expiry_to_ai_gateway_gemini_batch_files.rs"),
                    ),
                    (
                        "database/migrations/2026_11_27_000020_add_parts_to_ai_gateway_gemini_batch_files.rs",
                        include_bytes!("../database/migrations/2026_11_27_000020_add_parts_to_ai_gateway_gemini_batch_files.rs"),
                    ),
                ],
                "ai-gateway-migrations",
            )?;
            self.load_bundled_translations_from(
                app,
                localization::RESOURCES,
                localization::NAMESPACE,
            )
            .await?;
            self.commands(app, commands::register);
            let repository = app.config()?;
            let config = repository.get("ai_gateway");
            let managed = repository
                .get("ai_gateway.managed_keys")
                .filter(|value| !value.is_null())
                .map(serde_json::from_value::<DatabaseSettings>)
                .transpose()?
                .unwrap_or_default();
            if managed.enabled {
                self.load_migrations_with_application_overrides(app, KEY_MIGRATIONS)
                    .await?;
            }
            let usage = repository
                .get("ai_gateway.usage")
                .filter(|value| !value.is_null())
                .map(serde_json::from_value::<DatabaseSettings>)
                .transpose()?
                .unwrap_or_default();
            if usage.enabled {
                self.load_migrations_with_application_overrides(app, USAGE_MIGRATIONS)
                    .await?;
            }
            if config
                .as_ref()
                .and_then(|config| config.get("enabled"))
                .is_some_and(|enabled| enabled != &serde_json::Value::Bool(false))
            {
                let environment = app.make::<Environment>().await?;
                let ai = app.make::<openrust_ai::AiManager>().await?;
                let logger = app.make::<LogManager>().await?.channel(
                    config
                        .as_ref()
                        .and_then(|config| config.get("usage_channel"))
                        .and_then(serde_json::Value::as_str),
                )?;
                let store = if managed.enabled {
                    let connection = app
                        .make::<DatabaseManager>()
                        .await?
                        .connection(managed.connection.as_deref())?;
                    Some(KeyStore::new(connection))
                } else {
                    None
                };
                let usage_store = if usage.enabled {
                    let connection = app
                        .make::<DatabaseManager>()
                        .await?
                        .connection(usage.connection.as_deref())?;
                    Some(UsageStore::new(connection))
                } else {
                    None
                };
                if let Some(gateway) =
                    Gateway::configure(config, &environment, &ai, logger, store, usage_store)?
                {
                    let unversioned_paths = gateway.unversioned_paths;
                    app.container().instance(Arc::new(gateway))?;
                    self.load_routes_from(app, protocol::routes)?;
                    if unversioned_paths {
                        self.load_routes_from(app, protocol::unversioned_routes)?;
                    }
                }
            }
            Ok(())
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(AiGatewayServiceProvider)]
}
