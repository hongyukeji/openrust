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
mod conversation_store;
mod conversation_stream;
mod conversations;
mod embedding_cache;
mod embeddings;
mod error;
mod facade;
mod fake;
mod fake_embedding_gateway;
mod gemini;
mod manager;
mod messages;
mod middleware;
mod models;
mod native_tool_stream;
mod on_demand;
mod reranking;
mod streaming;
mod structured;
mod text;
mod tool_protocol;
mod tool_stream;
mod tool_stream_protocol;
mod tools;

pub use agent::{Agent, AnonymousAgent, PromptOptions, agent};
pub use configuration::load as default_config;
pub use conversation_store::{
    ConversationPage, ConversationParticipant, DatabaseConversationStore, StoredMessage,
};
pub use conversations::{ConversationResponse, PendingConversation, RemembersConversations};
pub use embeddings::{
    EmbeddingProvider, Embeddings, EmbeddingsPrompt, EmbeddingsResponse,
    PendingEmbeddingsGeneration,
};
pub use error::{AiError, AiResult};
pub use facade::Ai;
pub use fake::{FakeTextGateway, RecordedPrompt};
pub use fake_embedding_gateway::{FakeEmbeddingGateway, RecordedEmbeddings};
pub use manager::AiManager;
pub use messages::{AgentPrompt, Message, MessageRole};
pub use middleware::{AgentMiddleware, NextStep, PendingStep};
pub use models::{ModelCatalog, ModelInfo};
pub use reranking::{
    PendingReranking, RankedDocument, Reranking, RerankingPrompt, RerankingProvider,
    RerankingResponse,
};
pub use streaming::{
    AgentEventStream, AgentUserInteractionProtocol, StreamEvent, StreamEventData, StreamProtocol,
    StreamableAgentResponse, VercelDataProtocol,
};
pub use structured::{
    HasStructuredOutput, StructuredAgentResponse, StructuredAnonymousAgent, StructuredOutput,
};
pub use text::{AgentResponse, TextEndpoint, TextPrompt, TextProvider, Usage};
pub use tools::{
    StepEvent, StepEventStream, StepResponse, StepTextGateway, Tool, ToolCall, ToolDefinition,
    ToolRequest, ToolResult, ToolStep,
};

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
            let owner = Arc::new(());
            let scope_owner = owner.clone();
            app.container().scoped(move |_| {
                Ok(Arc::new(on_demand::OnDemandProviders::new(
                    scope_owner.clone(),
                )))
            })?;
            app.container().singleton(move |container| {
                Ok(Arc::new(
                    AiManager::with_http(
                        container.get("config")?,
                        container.make()?,
                        container.make()?,
                    )
                    .with_scope_owner(owner.clone()),
                ))
            })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.commands(app, console::register);
            let config = &[("config/ai.rs", include_bytes!("../config/ai.rs").as_slice())];
            self.publishes(app, config, "ai")?;
            self.publishes(app, config, "ai-config")?;
            self.publishes_migrations(
                app,
                &[(
                    "database/migrations/2026_01_11_000001_create_agent_conversations_table.rs",
                    include_bytes!("../database/migrations/2026_01_11_000001_create_agent_conversations_table.rs"),
                )],
                "ai-migrations",
            )?;
            self.publishes(app, console::STUBS, "ai-stubs")
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(AiServiceProvider)]
}
