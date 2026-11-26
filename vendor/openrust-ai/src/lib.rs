//! Laravel 风格的 AI SDK。
#![forbid(unsafe_code)]

use std::sync::Arc;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::query::embeddings::Embeddings as QueryEmbeddings,
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

mod agent;
mod agent_broadcast;
mod agent_queue;
mod agent_tool;
mod anthropic;
mod approval_store;
mod approval_turn;
mod approvals;
mod attachment_storage;
mod attachments;
mod audio;
pub mod classification;
#[path = "../config/ai.rs"]
mod configuration;
mod connection;
mod console;
mod conversation_store;
mod conversation_stream;
mod conversation_title;
mod conversations;
mod database_embeddings;
mod embedding_cache;
mod embeddings;
mod error;
pub mod events;
mod facade;
mod fake;
mod fake_embedding_gateway;
mod fake_reranking_gateway;
mod files;
mod gemini;
mod generation_options;
mod image;
mod load_skill;
mod localization;
mod manager;
mod mcp_tool;
mod media_storage;
mod messages;
mod middleware;
mod models;
mod native_tool_stream;
mod on_demand;
mod provider_options;
mod provider_replay;
mod request_context;
mod reranking;
mod similarity_search;
mod skill;
mod step;
mod stores;
mod streaming;
mod structured;
mod summarize;
mod text;
mod tool_choice;
mod tool_protocol;
mod tool_stream;
mod tool_stream_protocol;
mod tools;
mod transcription;
mod usage;

pub use agent::{Agent, AnonymousAgent, PromptOptions, ProviderTarget, agent};
pub use agent_queue::{AgentQueueCatch, AgentQueueThen, QueueableAgent, QueuedAgentResponse};
pub use agent_tool::{AgentTool, CanActAsTool};
pub use approval_turn::{ApprovalResponse, PendingToolApprovals};
pub use approvals::{Approval, Decision, Decisions, PendingApproval};
pub use attachments::{Attachment, DocumentAttachment};
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
    ConversationMessageStatus, ConversationPage, ConversationParticipant, ConversationStore,
    DatabaseConversationStore, StoredMessage,
};
pub use conversation_title::ConversationTitleAgent;
pub use conversations::{ConversationResponse, PendingConversation, RemembersConversations};
pub use embeddings::{
    EmbeddingProvider, Embeddings, EmbeddingsPrompt, EmbeddingsResponse,
    PendingEmbeddingsGeneration, ToEmbeddings,
};
pub use error::{AiError, AiResult};
pub use facade::Ai;
pub use fake::{FakeTextGateway, FakeTextResult, RecordedPrompt, RecordedQueuedPrompt};
pub use fake_embedding_gateway::{FakeEmbeddingGateway, RecordedEmbeddings};
pub use fake_reranking_gateway::{FakeRerankingGateway, FakeRerankingResult, RecordedReranking};
pub use files::{
    FakeFileGateway, FileInfo, FilePage, FileProvider, FileResponse, FileUpload, Files,
    ProviderFilePage, RecordedFileDeletion, RecordedFileUpload,
};
pub use generation_options::TextGenerationOptions;
pub use image::{
    GeneratedImage, Image, ImageAttachment, ImageFormat, ImagePrompt, ImageProvider, ImageResponse,
    ImageUsage, PendingImageGeneration,
};
pub use load_skill::LoadSkill;
pub use manager::AiManager;
pub use mcp_tool::{McpServerTool, McpTool};
pub use messages::{AgentPrompt, Message, MessageRole};
pub use middleware::{AgentMiddleware, NextStep, NextStreamStep, PendingStep, StepResult};
pub use models::{ModelCatalog, ModelInfo};
pub use provider_options::ProviderOptions;
pub use request_context::{INVOCATION_HEADER, MODEL_TURN_HEADER};
pub use reranking::{
    PendingReranking, RankedDocument, Reranking, RerankingOptions, RerankingPrompt,
    RerankingProvider, RerankingResponse, Reranks,
};
pub use similarity_search::SimilaritySearch;
pub use skill::{Skill, SkillSource};
pub use step::{FinishReason, Step};
pub use stores::{
    FakeStoreGateway, ProviderStoreFilePage, ProviderStorePage, RecordedStoreCreation,
    RecordedStoreDeletion, Store, StoreFile, StoreFileCounts, StoreFileInfo, StoreFilePage,
    StoreInfo, StoreOptions, StorePage, StoreProvider, Stores,
};
pub use streaming::{
    AgentEventStream, AgentUserInteractionProtocol, ReasoningPart, StreamEvent, StreamEventData,
    StreamProtocol, StreamableAgentResponse, StreamedAgentResponse, UrlCitation,
    VercelDataProtocol,
};
pub use structured::{
    HasStructuredOutput, StructuredAgentResponse, StructuredAnonymousAgent, StructuredOutput,
};
pub use summarize::{SummarizeAgent, Summarizes, SummarizesText};
pub use text::{AgentResponse, Meta, TextEndpoint, TextModelTier, TextPrompt, TextProvider};
pub use tool_choice::ToolChoice;
pub use tools::{
    StepEvent, StepEventStream, StepResponse, StepTextGateway, Tool, ToolCall, ToolDefinition,
    ToolRequest, ToolResult, ToolStep,
};
pub use usage::Usage;

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
            app.container().singleton_if::<QueryEmbeddings, _>(|_| {
                Ok(Arc::new(
                    QueryEmbeddings::new(database_embeddings::DatabaseEmbeddingProvider)
                        .without_cache(),
                ))
            })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.load_bundled_translations_from(app, localization::RESOURCES, "ai")
                .await?;
            self.commands(app, console::register);
            let config = &[("config/ai.rs", include_bytes!("../config/ai.rs").as_slice())];
            self.publishes(app, config, "ai")?;
            self.publishes(app, config, "ai-config")?;
            self.publishes_migrations(
                app,
                &[
                    (
                        "database/migrations/2026_01_11_000001_create_agent_conversations_table.rs",
                        include_bytes!("../database/migrations/2026_01_11_000001_create_agent_conversations_table.rs"),
                    ),
                    (
                        "database/migrations/2026_01_11_000002_create_agent_tool_approvals_table.rs",
                        include_bytes!("../database/migrations/2026_01_11_000002_create_agent_tool_approvals_table.rs"),
                    ),
                ],
                "ai-migrations",
            )?;
            self.publishes(app, console::STUBS, "ai-stubs")
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(AiServiceProvider)]
}
