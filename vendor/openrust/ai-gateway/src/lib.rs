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

mod accounts;
mod commands;
mod compatibility;
#[path = "../config/ai_gateway.rs"]
mod configuration;
mod gateway;
#[path = "../database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs"]
mod key_migration;
mod managed_keys;
mod protocol;

pub use configuration::load as default_config;
use gateway::Gateway;
use managed_keys::KeyStore;
use serde::Deserialize;

const KEY_MIGRATION: &str = "database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs";
static KEY_MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "vendor/openrust/ai-gateway/database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs",
    &key_migration::MIGRATION,
)];

#[derive(Default, Deserialize)]
struct ManagedKeysSettings {
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
                &[(
                    KEY_MIGRATION,
                    include_bytes!(
                        "../database/migrations/2026_11_26_000001_create_ai_gateway_keys_table.rs",
                    ),
                )],
                "ai-gateway-migrations",
            )?;
            self.commands(app, commands::register);
            let repository = app.config()?;
            let config = repository.get("ai_gateway");
            let managed = repository
                .get("ai_gateway.managed_keys")
                .filter(|value| !value.is_null())
                .map(serde_json::from_value::<ManagedKeysSettings>)
                .transpose()?
                .unwrap_or_default();
            if managed.enabled {
                self.load_migrations_with_application_overrides(app, KEY_MIGRATIONS)
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
                if let Some(gateway) = Gateway::configure(config, &environment, &ai, logger, store)?
                {
                    app.container().instance(Arc::new(gateway))?;
                    self.load_routes_from(app, protocol::routes)?;
                }
            }
            Ok(())
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(AiGatewayServiceProvider)]
}
