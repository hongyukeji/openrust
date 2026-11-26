//! Laravel 风格的 AI SDK。
#![forbid(unsafe_code)]

use std::sync::Arc;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

mod agent;
mod anthropic;
#[path = "../config/ai.rs"]
mod configuration;
mod connection;
mod console;
mod error;
mod facade;
mod fake;
mod gemini;
mod manager;
mod models;
mod streaming;
mod text;

pub use agent::{Agent, AnonymousAgent, PromptOptions, agent};
pub use configuration::load as default_config;
pub use error::{AiError, AiResult};
pub use facade::Ai;
pub use fake::{FakeTextGateway, RecordedPrompt};
pub use manager::AiManager;
pub use models::{ModelCatalog, ModelInfo};
pub use streaming::{AgentEventStream, StreamEvent, StreamEventData, StreamableAgentResponse};
pub use text::{AgentResponse, TextEndpoint, TextPrompt, TextProvider, Usage};

pub struct AiServiceProvider;

impl ServiceProvider for AiServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let environment = app.make::<openrust::config::Environment>().await?;
                if app.config()?.get("ai").is_none() {
                    self.merge_config_from(app, default_config(&environment)?, "ai")?;
                }
            }
            app.container().singleton(|container| {
                Ok(Arc::new(AiManager::with_http(
                    container.get("config")?,
                    container.make()?,
                    container.make()?,
                )))
            })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.commands(app, console::register);
            let config = &[("config/ai.rs", include_bytes!("../config/ai.rs").as_slice())];
            self.publishes(app, config, "ai")?;
            self.publishes(app, config, "ai-config")
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(AiServiceProvider)]
}
