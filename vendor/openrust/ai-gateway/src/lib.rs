//! 可独立启用的 AI 协议网关。
#![forbid(unsafe_code)]

use std::sync::Arc;

use openrust::{
    config::Environment,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    log::LogManager,
    support::{ProviderResult, ServiceProvider},
};

mod compatibility;
#[path = "../config/ai_gateway.rs"]
mod configuration;
mod gateway;
mod protocol;

pub use configuration::load as default_config;
use gateway::Gateway;

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
            let repository = app.config()?;
            let config = repository.get("ai_gateway");
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
                if let Some(gateway) = Gateway::configure(config, &environment, &ai, logger)? {
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
