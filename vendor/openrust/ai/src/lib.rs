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
mod agent_broadcast;
mod agent_queue;
mod anthropic;
mod audio;
pub mod classification;
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
mod fake_reranking_gateway;
mod files;
mod gemini;
mod image;
mod manager;
mod media_storage;
mod messages;
mod middleware;
mod models;
mod native_tool_stream;
mod on_demand;
mod reranking;
mod stores;
mod streaming;
mod structured;
mod text;
mod tool_protocol;
mod tool_stream;
mod tool_stream_protocol;
mod tools;
mod transcription;

pub use agent::{Agent, AnonymousAgent, PromptOptions, agent};
pub use agent_queue::{AgentQueueCatch, AgentQueueThen, QueueableAgent, QueuedAgentResponse};
pub use audio::{
    Audio, AudioFormat, AudioPrompt, AudioProvider, AudioResponse, AudioVoice,
    PendingAudioGeneration, ToAudio,
};
pub use classification::{
    Classification, ClassificationPrompt, ClassificationProvider, ClassificationResponse,
    ClassificationUsage, DecideOptions, Decides, DecidesBetween, DecidesText,
    FakeClassificationGateway, FakeClassificationResult, PendingClassification,
    RecordedClassification,
};
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
pub use fake_reranking_gateway::{FakeRerankingGateway, FakeRerankingResult, RecordedReranking};
pub use files::{
    FakeFileGateway, FileInfo, FilePage, FileProvider, FileResponse, FileUpload, Files,
    ProviderFilePage, RecordedFileDeletion, RecordedFileUpload,
};
pub use image::{
    GeneratedImage, Image, ImageAttachment, ImageFormat, ImagePrompt, ImageProvider, ImageResponse,
    ImageUsage, PendingImageGeneration,
};
pub use manager::AiManager;
pub use messages::{AgentPrompt, Message, MessageRole};
pub use middleware::{AgentMiddleware, NextStep, PendingStep};
pub use models::{ModelCatalog, ModelInfo};
pub use reranking::{
    PendingReranking, RankedDocument, Reranking, RerankingOptions, RerankingPrompt,
    RerankingProvider, RerankingResponse, Reranks,
};
pub use stores::{
    FakeStoreGateway, ProviderStoreFilePage, ProviderStorePage, RecordedStoreCreation,
    RecordedStoreDeletion, Store, StoreFile, StoreFileCounts, StoreFileInfo, StoreFilePage,
    StoreInfo, StoreOptions, StorePage, StoreProvider, Stores,
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

pub use transcription::{
    PendingTranscriptionGeneration, Transcription, TranscriptionAudio, TranscriptionPrompt,
    TranscriptionProvider, TranscriptionResponse, TranscriptionSegment, TranscriptionUsage,
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
