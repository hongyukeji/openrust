use std::{sync::Arc, time::Duration};

use async_openai::types::{
    chat::{
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartFile,
        ChatCompletionRequestMessageContentPartImage, ChatCompletionRequestMessageContentPartText,
        ChatCompletionRequestSystemMessage, ChatCompletionRequestSystemMessageContent,
        ChatCompletionRequestToolMessage, ChatCompletionRequestToolMessageContent,
        ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
        ChatCompletionRequestUserMessageContentPart, ChatCompletionStreamOptions,
        CreateChatCompletionRequest, FileObject, FunctionCall, ImageUrl, ResponseFormat,
        ResponseFormatJsonSchema,
    },
    responses::{
        CreateResponse, EasyInputContent, EasyInputMessage, FunctionCallOutput,
        FunctionCallOutputItemParam, FunctionToolCall, InputContent, InputFileContent,
        InputImageContent, InputItem, InputParam, InputTextContent, Item, ResponseTextParam, Role,
        TextResponseFormatConfiguration,
    },
};
use openrust::{
    config::Environment,
    http::{
        HttpFuture,
        client::{self, Factory, Part, Response},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AgentPrompt, AiError, AiResult, Attachment, EmbeddingProvider, FinishReason, MessageRole,
    ModelCatalog, RerankingProvider, Step, StepEventStream, StepResponse, StepTextGateway,
    StreamableAgentResponse, StructuredOutput, TextGenerationOptions, ToolCall, ToolChoice,
    ToolDefinition, ToolResult, ToolStep, UrlCitation, Usage,
    audio::{AudioProvider, MistralAudioProvider, OpenAiAudioProvider},
    classification::{
        ClassificationProvider, DecisionsProtocol, HttpDecisionsProvider,
        OpenAiClassificationProvider,
    },
    connection::{Authorization, Connection},
    embeddings::OpenAiEmbeddingProvider,
    files::{FileProvider, OpenAiFileProvider, WorkspaceFileProtocol, WorkspaceFileProvider},
    image::{ImageProvider, OpenAiImageProvider},
    models::ListedModel,
    provider_options::apply_provider_options,
    reranking::{HttpRerankingProvider, RerankingProtocol},
    stores::StoreProvider,
    transcription::{HttpTranscriptionProvider, TranscriptionProtocol, TranscriptionProvider},
    usage::OpenAiUsage,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentResponse {
    pub text: String,
    pub reasoning: String,
    pub usage: Usage,
    pub provider: String,
    pub model: String,
    pub citations: Vec<UrlCitation>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub steps: Vec<Step>,
}

/// 模型响应的来源连接、模型以及引文。
#[derive(Debug, Serialize)]
pub struct Meta<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub citations: &'a [UrlCitation],
}

impl AgentResponse {
    pub fn new(
        text: impl Into<String>,
        usage: Usage,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            text: text.into(),
            reasoning: String::new(),
            usage,
            provider: provider.into(),
            model: model.into(),
            citations: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            steps: Vec::new(),
        }
    }

    pub fn meta(&self) -> Meta<'_> {
        Meta {
            provider: &self.provider,
            model: &self.model,
            citations: &self.citations,
        }
    }

    pub(crate) fn with_step(mut self, finish_reason: FinishReason) -> Self {
        self.steps.push(self.public_step(finish_reason));
        self
    }

    pub(crate) fn into_completed_steps(mut self) -> Option<Self> {
        let (first, remaining) = self.steps.split_first()?;
        let last = remaining.last().unwrap_or(first);
        self.provider.clone_from(&last.provider);
        self.model.clone_from(&last.model);
        self.text.clone_from(&last.text);
        self.reasoning.clone_from(&last.reasoning);
        self.citations.clone_from(&last.citations);
        self.usage = remaining.iter().fold(first.usage.clone(), |usage, step| {
            step.usage.clone().combine(&usage)
        });
        self.tool_calls = self
            .steps
            .iter()
            .flat_map(|step| step.tool_calls.iter().cloned())
            .collect();
        self.tool_results = self
            .steps
            .iter()
            .flat_map(|step| step.tool_results.iter().cloned())
            .collect();
        Some(self)
    }

    pub(crate) fn into_completed_tool_steps(mut self) -> Option<Self> {
        let last_tool_step = self
            .steps
            .iter()
            .rposition(|step| !step.tool_results.is_empty())?;
        self.steps.truncate(last_tool_step + 1);
        self.into_completed_steps()
    }

    pub(crate) fn public_step(&self, finish_reason: FinishReason) -> Step {
        Step {
            text: self.text.clone(),
            tool_calls: self.tool_calls.clone(),
            tool_results: self.tool_results.clone(),
            finish_reason,
            usage: self.usage.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            citations: self.citations.clone(),
            reasoning: self.reasoning.clone(),
            provider_replay: None,
        }
    }
}

pub struct TextPrompt<'a> {
    pub instructions: &'a str,
    pub prompt: &'a str,
    pub model: &'a str,
    pub timeout: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextModelTier {
    #[default]
    Default,
    Cheapest,
    Smartest,
}

pub trait TextProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_text_model(&self) -> AiResult<&str>;

    fn text_model(&self, _tier: TextModelTier) -> AiResult<&str> {
        self.default_text_model()
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>>;

    fn stream_text<'a>(
        &'a self,
        _prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        Box::pin(async { Err(AiError::StreamingNotSupported) })
    }

    fn generate<'a>(&'a self, prompt: AgentPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        if prompt.structured_output.is_some() {
            return Box::pin(async { Err(AiError::StructuredOutputNotSupported) });
        }
        if !prompt.attachments.is_empty() {
            return Box::pin(async { Err(AiError::AttachmentsNotSupported) });
        }
        if prompt.options != TextGenerationOptions::default() {
            return Box::pin(async { Err(AiError::GenerationOptionsNotSupported) });
        }
        if prompt
            .provider_options
            .is_some_and(|options| !options.is_empty())
        {
            return Box::pin(async { Err(AiError::ProviderOptionsNotSupported) });
        }
        if prompt.messages.is_empty() {
            self.generate_text(prompt.text)
        } else {
            Box::pin(async { Err(AiError::MessagesNotSupported) })
        }
    }

    fn stream<'a>(
        &'a self,
        prompt: AgentPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        if prompt.structured_output.is_some() {
            return Box::pin(async { Err(AiError::StructuredOutputNotSupported) });
        }
        if !prompt.attachments.is_empty() {
            return Box::pin(async { Err(AiError::AttachmentsNotSupported) });
        }
        if prompt.options != TextGenerationOptions::default() {
            return Box::pin(async { Err(AiError::GenerationOptionsNotSupported) });
        }
        if prompt
            .provider_options
            .is_some_and(|options| !options.is_empty())
        {
            return Box::pin(async { Err(AiError::ProviderOptionsNotSupported) });
        }
        if prompt.messages.is_empty() {
            self.stream_text(prompt.text)
        } else {
            Box::pin(async { Err(AiError::MessagesNotSupported) })
        }
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async {
            ModelCatalog::merge(Err(AiError::ModelsNotSupported), std::iter::empty())
        })
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        None
    }

    fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        None
    }

    fn audio_provider(&self) -> Option<Arc<dyn AudioProvider>> {
        None
    }

    fn transcription_provider(&self) -> Option<Arc<dyn TranscriptionProvider>> {
        None
    }

    fn image_provider(&self) -> Option<Arc<dyn ImageProvider>> {
        None
    }

    fn file_provider(&self) -> Option<Arc<dyn FileProvider>> {
        None
    }

    fn store_provider(&self) -> Option<Arc<dyn StoreProvider>> {
        None
    }

    fn classification_provider(&self) -> Option<Arc<dyn ClassificationProvider>> {
        None
    }

    fn reranking_provider(&self) -> Option<Arc<dyn RerankingProvider>> {
        None
    }

    fn supports_endpoint(&self, _endpoint: TextEndpoint) -> bool {
        false
    }

    /// 向已配置的协议连接发送 JSON。调用方负责入站鉴权和模型授权。
    fn relay(&self, _endpoint: TextEndpoint, _body: Value) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn relay_multipart(
        &self,
        _endpoint: TextEndpoint,
        _body: Value,
        _parts: Vec<(String, Part)>,
    ) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEndpoint {
    ChatCompletions,
    Responses,
    CountResponseTokens,
    Messages,
    CountMessageTokens,
    Embeddings,
    ImagesGenerations,
    ImagesEdits,
    GeminiGenerateContent,
    GeminiStreamGenerateContent,
    GeminiCountTokens,
    GeminiEmbedContent,
    GeminiBatchEmbedContents,
}

impl TextEndpoint {
    pub fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat/completions",
            Self::Responses => "responses",
            Self::CountResponseTokens => "responses/input_tokens",
            Self::Messages => "messages",
            Self::CountMessageTokens => "messages/count_tokens",
            Self::Embeddings => "embeddings",
            Self::ImagesGenerations => "images/generations",
            Self::ImagesEdits => "images/edits",
            Self::GeminiGenerateContent => "generateContent",
            Self::GeminiStreamGenerateContent => "streamGenerateContent",
            Self::GeminiCountTokens => "countTokens",
            Self::GeminiEmbedContent => "embedContent",
            Self::GeminiBatchEmbedContents => "batchEmbedContents",
        }
    }

    pub const fn is_gemini_embedding(self) -> bool {
        matches!(
            self,
            Self::GeminiEmbedContent | Self::GeminiBatchEmbedContents
        )
    }

    pub const fn is_image(self) -> bool {
        matches!(self, Self::ImagesGenerations | Self::ImagesEdits)
    }

    pub const fn is_counting(self) -> bool {
        matches!(
            self,
            Self::CountResponseTokens | Self::CountMessageTokens | Self::GeminiCountTokens
        )
    }
}

#[derive(Clone, Copy)]
pub(crate) enum OpenAiProtocol {
    ChatCompletions,
    Mistral,
    Groq,
    Responses,
}

impl OpenAiProtocol {
    fn endpoint(self) -> TextEndpoint {
        match self {
            Self::ChatCompletions | Self::Mistral | Self::Groq => TextEndpoint::ChatCompletions,
            Self::Responses => TextEndpoint::Responses,
        }
    }
}

pub(crate) struct OpenAiTextProvider {
    protocol: OpenAiProtocol,
    connection: Arc<Connection>,
}

impl OpenAiTextProvider {
    pub(crate) fn new(
        protocol: OpenAiProtocol,
        config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> AiResult<Self> {
        let authorization = match config.get("auth") {
            None | Some(Value::Null) => Authorization::Bearer,
            Some(value) if value == "bearer" => Authorization::Bearer,
            Some(value) if value == "api-key" => Authorization::ApiKey,
            _ => return Err(AiError::InvalidConfiguration("providers.<name>.auth")),
        };
        Ok(Self {
            protocol,
            connection: Arc::new(Connection::new(config, environment, http, authorization)?),
        })
    }

    async fn list_models(&self) -> AiResult<Vec<ListedModel>> {
        #[derive(Deserialize)]
        struct ModelResponse {
            data: Vec<ListedModel>,
        }

        let response = validate(self.connection.request(None)?.get("models").await)?;
        let response: ModelResponse = response.json().map_err(|_| AiError::InvalidResponse)?;
        Ok(response.data)
    }

    fn chat_user_message(
        &self,
        text: &str,
        attachments: &[Attachment],
    ) -> AiResult<ChatCompletionRequestMessage> {
        let content = if attachments.is_empty() {
            ChatCompletionRequestUserMessageContent::Text(text.into())
        } else {
            use ChatCompletionRequestUserMessageContentPart::{File, ImageUrl as ImagePart, Text};
            let mut parts = Vec::new();
            if !text.trim().is_empty() {
                parts.push(Text(ChatCompletionRequestMessageContentPartText {
                    text: text.into(),
                    ..Default::default()
                }));
            }
            for attachment in attachments {
                parts.push(match attachment {
                    Attachment::Image(image) => {
                        ImagePart(ChatCompletionRequestMessageContentPartImage {
                            image_url: ImageUrl {
                                url: image.data_url(),
                                detail: None,
                            },
                            ..Default::default()
                        })
                    }
                    Attachment::Document(document)
                        if (document.is_pdf()
                            && !matches!(self.protocol, OpenAiProtocol::Groq))
                            || matches!(self.protocol, OpenAiProtocol::Mistral) =>
                    {
                        File(ChatCompletionRequestMessageContentPartFile {
                            file: FileObject {
                                filename: Some(document.filename().into()),
                                file_data: Some(document.data_url()),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                    }
                    Attachment::Document(_) => return Err(AiError::AttachmentsNotSupported),
                });
            }
            ChatCompletionRequestUserMessageContent::Array(parts)
        };
        Ok(ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content,
                name: None,
            },
        ))
    }

    // OpenAI 兼容驱动沿用 Chat 兼容端点的 max_tokens 契约。
    #[allow(deprecated)]
    fn chat_body(
        &self,
        prompt: &AgentPrompt<'_>,
        stream: bool,
    ) -> AiResult<CreateChatCompletionRequest> {
        let mut messages = Vec::new();
        if !prompt.text.instructions.is_empty() {
            messages.push(ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage {
                    content: ChatCompletionRequestSystemMessageContent::Text(
                        prompt.text.instructions.into(),
                    ),
                    name: None,
                },
            ));
        }
        for message in prompt.messages {
            match message.role {
                MessageRole::User => {
                    messages.push(self.chat_user_message(&message.content, &message.attachments)?);
                }
                MessageRole::Assistant => {
                    let tools: Vec<_> = message
                        .tool_calls
                        .iter()
                        .map(|call| {
                            ChatCompletionMessageToolCalls::Function(
                                ChatCompletionMessageToolCall {
                                    id: call.wire_id().to_owned(),
                                    function: FunctionCall {
                                        name: call.name.clone(),
                                        arguments: Value::Object(call.arguments.clone())
                                            .to_string(),
                                    },
                                },
                            )
                        })
                        .collect();
                    messages.push(ChatCompletionRequestMessage::Assistant(
                        ChatCompletionRequestAssistantMessage {
                            content: (tools.is_empty() || !message.content.is_empty()).then(|| {
                                ChatCompletionRequestAssistantMessageContent::Text(
                                    message.content.clone(),
                                )
                            }),
                            tool_calls: (!tools.is_empty()).then_some(tools),
                            ..Default::default()
                        },
                    ));
                }
                MessageRole::ToolResult => {
                    messages.extend(message.tool_results.iter().map(|result| {
                        ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
                            content: ChatCompletionRequestToolMessageContent::Text(
                                result.result.clone(),
                            ),
                            tool_call_id: result.wire_id().to_owned(),
                        })
                    }));
                }
            }
        }
        messages.push(self.chat_user_message(prompt.text.prompt, prompt.attachments)?);
        Ok(CreateChatCompletionRequest {
            model: prompt.text.model.into(),
            messages,
            max_tokens: prompt.options.max_tokens,
            temperature: prompt.options.temperature,
            top_p: prompt.options.top_p,
            response_format: prompt.structured_output.as_ref().map(|output| {
                ResponseFormat::JsonSchema {
                    json_schema: schema_format(output),
                }
            }),
            stream: stream.then_some(true),
            stream_options: (stream
                && !matches!(
                    self.protocol,
                    OpenAiProtocol::Mistral | OpenAiProtocol::Groq
                ))
            .then_some(ChatCompletionStreamOptions {
                include_usage: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn responses_content(text: &str, attachments: &[Attachment]) -> EasyInputContent {
        if attachments.is_empty() {
            return EasyInputContent::Text(text.into());
        }
        let mut parts = Vec::new();
        if !text.trim().is_empty() {
            parts.push(InputContent::InputText(InputTextContent {
                text: text.into(),
                ..Default::default()
            }));
        }
        parts.extend(attachments.iter().map(|attachment| match attachment {
            Attachment::Image(image) => InputContent::InputImage(InputImageContent {
                image_url: Some(image.data_url()),
                ..Default::default()
            }),
            Attachment::Document(document) => InputContent::InputFile(InputFileContent {
                filename: Some(document.filename().into()),
                file_data: Some(document.data_url()),
                ..Default::default()
            }),
        }));
        EasyInputContent::ContentList(parts)
    }

    fn responses_body(&self, prompt: &AgentPrompt<'_>, stream: bool) -> CreateResponse {
        let input = if prompt.messages.is_empty() && prompt.attachments.is_empty() {
            InputParam::Text(prompt.text.prompt.into())
        } else {
            let message = |role, content| {
                InputItem::EasyMessage(EasyInputMessage {
                    r#type: Default::default(),
                    role,
                    content,
                    phase: None,
                })
            };
            let mut history = Vec::new();
            for entry in prompt.messages {
                match entry.role {
                    MessageRole::User => history.push(message(
                        Role::User,
                        Self::responses_content(&entry.content, &entry.attachments),
                    )),
                    MessageRole::Assistant => {
                        if entry.tool_calls.is_empty() || !entry.content.is_empty() {
                            history.push(message(
                                Role::Assistant,
                                EasyInputContent::Text(entry.content.clone()),
                            ));
                        }
                        history.extend(entry.tool_calls.iter().map(|call| {
                            InputItem::Item(Item::FunctionCall(FunctionToolCall {
                                arguments: Value::Object(call.arguments.clone()).to_string(),
                                call_id: call.wire_id().to_owned(),
                                name: call.name.clone(),
                                ..Default::default()
                            }))
                        }));
                    }
                    MessageRole::ToolResult => {
                        history.extend(entry.tool_results.iter().map(|result| {
                            InputItem::Item(Item::FunctionCallOutput(FunctionCallOutputItemParam {
                                call_id: Some(result.wire_id().to_owned()),
                                output: FunctionCallOutput::Text(result.result.clone()),
                                id: None,
                                status: None,
                                caller: None,
                                name: None,
                                namespace: None,
                            }))
                        }))
                    }
                }
            }
            history.push(message(
                Role::User,
                Self::responses_content(prompt.text.prompt, prompt.attachments),
            ));
            InputParam::Items(history)
        };
        CreateResponse {
            instructions: (!prompt.text.instructions.is_empty())
                .then(|| prompt.text.instructions.into()),
            input,
            text: prompt
                .structured_output
                .as_ref()
                .map(|output| ResponseTextParam {
                    format: TextResponseFormatConfiguration::JsonSchema(schema_format(output)),
                    verbosity: None,
                }),
            model: Some(prompt.text.model.into()),
            max_output_tokens: prompt.options.max_tokens,
            temperature: prompt.options.temperature,
            top_p: prompt.options.top_p,
            store: Some(self.connection.store()),
            stream: stream.then_some(true),
            ..Default::default()
        }
    }

    fn request_body(&self, prompt: &AgentPrompt<'_>, stream: bool) -> AiResult<Value> {
        let mut body = match self.protocol {
            OpenAiProtocol::ChatCompletions | OpenAiProtocol::Mistral | OpenAiProtocol::Groq => {
                serde_json::to_value(self.chat_body(prompt, stream)?)
            }
            OpenAiProtocol::Responses => serde_json::to_value(self.responses_body(prompt, stream)),
        }
        .map_err(|_| AiError::InvalidSchema)?;
        if matches!(self.protocol, OpenAiProtocol::Mistral) {
            mistral_document_parts(&mut body);
        }
        if matches!(self.protocol, OpenAiProtocol::Groq) {
            if let Some(tokens) = body
                .as_object_mut()
                .and_then(|body| body.remove("max_tokens"))
            {
                body["max_completion_tokens"] = tokens;
            }
            if stream {
                inline_groq_schema(&mut body)?;
            }
        }
        apply_provider_options(&mut body, prompt.provider_options, false)?;
        if matches!(self.protocol, OpenAiProtocol::Responses) {
            include_encrypted_reasoning(&mut body, prompt.text.model)?;
        }
        if prompt
            .messages
            .iter()
            .any(|message| message.provider_replay.is_some())
        {
            let scope = self
                .approval_replay_scope()?
                .ok_or(AiError::InvalidResponse)?;
            match self.protocol {
                OpenAiProtocol::ChatCompletions => {
                    restore_chat_reasoning(&mut body, prompt, &scope)
                }
                OpenAiProtocol::Mistral | OpenAiProtocol::Groq => {}
                OpenAiProtocol::Responses => restore_responses_items(&mut body, prompt, &scope)?,
            }
        }
        Ok(body)
    }

    async fn send_text(&self, prompt: &AgentPrompt<'_>, stream: bool) -> AiResult<Response> {
        let body = self.request_body(prompt, stream)?;
        self.send_sdk_body(&body, stream, prompt.text.timeout).await
    }

    async fn send_sdk_body(
        &self,
        body: &Value,
        stream: bool,
        timeout: Option<Duration>,
    ) -> AiResult<Response> {
        match self
            .connection
            .send_sdk_text(self.protocol.endpoint(), body, stream, timeout)
            .await
        {
            Err(AiError::ProviderStatus(498)) if matches!(self.protocol, OpenAiProtocol::Groq) => {
                Err(AiError::ProviderOverloaded(498))
            }
            result => result,
        }
    }
}

impl TextProvider for OpenAiTextProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        self.connection.default_text_model()
    }

    fn text_model(&self, tier: TextModelTier) -> AiResult<&str> {
        self.connection.text_model(tier)
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async { self.connection.catalog(self.list_models().await) })
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        Some(self)
    }

    fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        self.supports_endpoint(TextEndpoint::Embeddings).then(|| {
            Arc::new(OpenAiEmbeddingProvider::new(
                self.connection.clone(),
                matches!(self.protocol, OpenAiProtocol::Mistral),
            )) as Arc<dyn EmbeddingProvider>
        })
    }

    fn audio_provider(&self) -> Option<Arc<dyn AudioProvider>> {
        if matches!(self.protocol, OpenAiProtocol::Mistral) {
            return Some(Arc::new(MistralAudioProvider(self.connection.clone())));
        }
        (matches!(self.protocol, OpenAiProtocol::Responses) || self.connection.audio_configured())
            .then(|| {
                Arc::new(OpenAiAudioProvider(self.connection.clone())) as Arc<dyn AudioProvider>
            })
    }

    fn transcription_provider(&self) -> Option<Arc<dyn TranscriptionProvider>> {
        let protocol = match self.protocol {
            OpenAiProtocol::Mistral => TranscriptionProtocol::Mistral,
            OpenAiProtocol::Groq => TranscriptionProtocol::Groq,
            OpenAiProtocol::Responses => TranscriptionProtocol::OpenAi,
            OpenAiProtocol::ChatCompletions if self.connection.transcription_configured() => {
                TranscriptionProtocol::OpenAi
            }
            _ => return None,
        };
        Some(Arc::new(HttpTranscriptionProvider {
            connection: self.connection.clone(),
            protocol,
        }))
    }

    fn image_provider(&self) -> Option<Arc<dyn ImageProvider>> {
        (matches!(self.protocol, OpenAiProtocol::Responses) || self.connection.images_configured())
            .then(|| {
                Arc::new(OpenAiImageProvider(self.connection.clone())) as Arc<dyn ImageProvider>
            })
    }

    fn file_provider(&self) -> Option<Arc<dyn FileProvider>> {
        if self.connection.uses_openrouter_files() {
            return Some(Arc::new(WorkspaceFileProvider {
                connection: self.connection.clone(),
                protocol: WorkspaceFileProtocol::OpenRouter,
            }));
        }
        (matches!(self.protocol, OpenAiProtocol::Responses) || self.connection.files_configured())
            .then(|| Arc::new(OpenAiFileProvider(self.connection.clone())) as Arc<dyn FileProvider>)
    }

    fn store_provider(&self) -> Option<Arc<dyn StoreProvider>> {
        (matches!(self.protocol, OpenAiProtocol::Responses) || self.connection.stores_configured())
            .then(|| {
                Arc::new(OpenAiFileProvider(self.connection.clone())) as Arc<dyn StoreProvider>
            })
    }

    fn classification_provider(&self) -> Option<Arc<dyn ClassificationProvider>> {
        if self.connection.uses_openrouter_decisions() {
            return Some(Arc::new(HttpDecisionsProvider {
                connection: self.connection.clone(),
                protocol: DecisionsProtocol::OpenRouter,
            }));
        }

        (matches!(self.protocol, OpenAiProtocol::Responses)
            || self.connection.classification_configured())
        .then(|| {
            Arc::new(OpenAiClassificationProvider(self.connection.clone()))
                as Arc<dyn ClassificationProvider>
        })
    }

    fn reranking_provider(&self) -> Option<Arc<dyn RerankingProvider>> {
        self.connection.reranking_configured().then(|| {
            Arc::new(HttpRerankingProvider {
                connection: self.connection.clone(),
                protocol: RerankingProtocol::TopN,
            }) as Arc<dyn RerankingProvider>
        })
    }

    fn supports_endpoint(&self, endpoint: TextEndpoint) -> bool {
        match endpoint {
            TextEndpoint::Embeddings => {
                matches!(
                    self.protocol,
                    OpenAiProtocol::Responses | OpenAiProtocol::Mistral
                ) || self.connection.embeddings_configured()
            }
            TextEndpoint::ImagesGenerations | TextEndpoint::ImagesEdits => {
                matches!(self.protocol, OpenAiProtocol::Responses)
                    || self.connection.images_configured()
            }
            TextEndpoint::CountResponseTokens => matches!(self.protocol, OpenAiProtocol::Responses),
            TextEndpoint::ChatCompletions => true,
            _ => self.protocol.endpoint() == endpoint,
        }
    }

    fn relay(&self, endpoint: TextEndpoint, body: Value) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async move {
            if !self.supports_endpoint(endpoint) {
                return Err(AiError::EndpointNotSupported);
            }
            let stream = !endpoint.is_counting()
                && endpoint != TextEndpoint::Embeddings
                && body.get("stream").and_then(Value::as_bool) == Some(true);
            self.connection.send(endpoint, &body, stream, None).await
        })
    }

    fn relay_multipart(
        &self,
        endpoint: TextEndpoint,
        body: Value,
        parts: Vec<(String, Part)>,
    ) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async move {
            if endpoint != TextEndpoint::ImagesEdits || !self.supports_endpoint(endpoint) {
                return Err(AiError::EndpointNotSupported);
            }
            let mut request = self.connection.request(None)?.as_multipart();
            if body["stream"].as_str() == Some("true") {
                request = request.stream();
            }
            for (field, part) in parts {
                request = request.attach(field, part);
            }
            request
                .post(endpoint.path(), &body)
                .await
                .map_err(transport_error)
        })
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>> {
        self.generate(prompt.into())
    }

    fn stream_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        self.stream(prompt.into())
    }

    fn generate<'a>(&'a self, prompt: AgentPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async move {
            let response = self.send_text(&prompt, false).await?;
            let result = match self.protocol {
                OpenAiProtocol::ChatCompletions => parse_chat(response)?,
                OpenAiProtocol::Mistral => {
                    parse_native_chat(response, crate::tool_protocol::mistral_step)?
                }
                OpenAiProtocol::Groq => {
                    parse_native_chat(response, crate::tool_protocol::groq_step)?
                }
                OpenAiProtocol::Responses => parse_responses(response)?,
            };
            Ok(AgentResponse {
                text: result.text,
                reasoning: result.reasoning,
                usage: result.usage,
                provider: self.name().into(),
                model: result.model.unwrap_or_else(|| prompt.text.model.into()),
                citations: result.citations,
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                steps: Vec::new(),
            }
            .with_step(result.finish_reason))
        })
    }
    fn stream<'a>(
        &'a self,
        prompt: AgentPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        Box::pin(async move {
            if matches!(
                self.protocol,
                OpenAiProtocol::Mistral | OpenAiProtocol::Groq
            ) {
                let events = self.stream_text_step(&prompt, &[], &[]).await?;
                let events = crate::StepResult::Stream(events)
                    .probe()
                    .await?
                    .into_stream();
                return Ok(crate::tool_stream::single_step_response(
                    Arc::new(Self {
                        protocol: self.protocol,
                        connection: self.connection.clone(),
                    }),
                    prompt,
                    events,
                ));
            }
            let response = self.send_text(&prompt, true).await?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            Ok(StreamableAgentResponse::from_upstream(
                self.protocol.endpoint(),
                response.into_stream().map_err(transport_error)?,
                self.name().into(),
                prompt.text.model.to_owned(),
            ))
        })
    }
}

impl OpenAiTextProvider {
    fn tool_payload(
        &self,
        prompt: &AgentPrompt<'_>,
        tools: &[ToolDefinition],
        history: &[ToolStep],
        stream: bool,
    ) -> AiResult<Value> {
        let body = self.request_body(prompt, stream)?;
        let mut body = match self.protocol {
            OpenAiProtocol::ChatCompletions | OpenAiProtocol::Mistral | OpenAiProtocol::Groq => {
                crate::tool_protocol::chat_request(body, tools, history)?
            }
            OpenAiProtocol::Responses => {
                crate::tool_protocol::responses_request(body, tools, history)?
            }
        };
        if tools.is_empty()
            && (stream
                || matches!(
                    self.protocol,
                    OpenAiProtocol::Mistral | OpenAiProtocol::Groq
                ))
        {
            body.as_object_mut()
                .ok_or(AiError::InvalidResponse)?
                .remove("tools");
        }
        if !tools.is_empty() && matches!(self.protocol, OpenAiProtocol::Groq) {
            inline_groq_schema(&mut body)?;
        }
        let choice = prompt.tool_choice.or(matches!(
            self.protocol,
            OpenAiProtocol::Mistral | OpenAiProtocol::Groq
        )
        .then_some(&ToolChoice::Auto));
        if let Some(choice) = choice.filter(|_| !tools.is_empty()) {
            body["tool_choice"] = match choice {
                ToolChoice::Auto => "auto".into(),
                ToolChoice::None => "none".into(),
                ToolChoice::Required => "required".into(),
                ToolChoice::Tool(name) => match self.protocol {
                    OpenAiProtocol::ChatCompletions
                    | OpenAiProtocol::Mistral
                    | OpenAiProtocol::Groq => {
                        json!({"type":"function","function":{"name":name}})
                    }
                    OpenAiProtocol::Responses => json!({"type":"function","name":name}),
                },
            };
        }
        Ok(body)
    }
}

impl StepTextGateway for OpenAiTextProvider {
    fn approval_replay_scope(&self) -> AiResult<Option<String>> {
        let protocol = match self.protocol {
            OpenAiProtocol::ChatCompletions => "chat-tool-replay",
            OpenAiProtocol::Mistral => "mistral-chat-tool-replay",
            OpenAiProtocol::Groq => "groq-chat-tool-replay",
            OpenAiProtocol::Responses => "responses-tool-replay",
        };
        self.connection.request_namespace(protocol).map(Some)
    }

    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        Box::pin(async move {
            let payload = self.tool_payload(prompt, tools, history, false)?;
            let response = self
                .send_sdk_body(&payload, false, prompt.text.timeout)
                .await?;
            let response = response
                .json::<Value>()
                .map_err(|_| AiError::InvalidResponse)?;
            match self.protocol {
                OpenAiProtocol::ChatCompletions => crate::tool_protocol::chat_step(response),
                OpenAiProtocol::Mistral => crate::tool_protocol::mistral_step(response),
                OpenAiProtocol::Groq => crate::tool_protocol::groq_step(response),
                OpenAiProtocol::Responses => crate::tool_protocol::responses_step(response),
            }
        })
    }
    fn stream_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepEventStream>> {
        Box::pin(async move {
            let payload = self.tool_payload(prompt, tools, history, true)?;
            let response = self
                .send_sdk_body(&payload, true, prompt.text.timeout)
                .await?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            let stream = response.into_stream().map_err(transport_error)?;
            Ok(crate::tool_stream_protocol::openai(
                self.protocol,
                stream,
                prompt.structured_output.is_some(),
            ))
        })
    }
}

fn restore_chat_reasoning(body: &mut Value, prompt: &AgentPrompt<'_>, scope: &str) {
    let mut index = usize::from(!prompt.text.instructions.is_empty());
    for message in prompt.messages {
        if message.role == MessageRole::Assistant
            && let Some(reasoning) = message
                .provider_replay
                .as_ref()
                .and_then(|replay| replay.chat_reasoning(scope, prompt.text.model))
        {
            body["messages"][index]["reasoning_content"] = reasoning.into();
        }
        index += if message.role == MessageRole::ToolResult {
            message.tool_results.len()
        } else {
            1
        };
    }
}

fn restore_responses_items(
    body: &mut Value,
    prompt: &AgentPrompt<'_>,
    scope: &str,
) -> AiResult<()> {
    let input = body["input"]
        .as_array_mut()
        .ok_or(AiError::InvalidResponse)?;
    let mut index = 0;
    for message in prompt.messages {
        let count = match message.role {
            MessageRole::User => 1,
            MessageRole::Assistant => {
                message.tool_calls.len()
                    + usize::from(message.tool_calls.is_empty() || !message.content.is_empty())
            }
            MessageRole::ToolResult => message.tool_results.len(),
        };
        if message.role == MessageRole::Assistant
            && let Some(items) = message
                .provider_replay
                .as_ref()
                .and_then(|replay| replay.responses_items(scope, prompt.text.model))
        {
            let range = index..index + count;
            if input.get(range.clone()).is_none() {
                return Err(AiError::InvalidResponse);
            }
            input.splice(range, items.iter().cloned());
            index += items.len();
        } else {
            index += count;
        }
    }
    Ok(())
}

fn include_encrypted_reasoning(body: &mut Value, model: &str) -> AiResult<()> {
    const ENCRYPTED: &str = "reasoning.encrypted_content";
    let known_model = (model.starts_with("gpt-5") && !model.starts_with("gpt-5-chat"))
        || ["gpt-6", "o4-mini", "o3", "o1"]
            .iter()
            .any(|prefix| model.starts_with(prefix));
    if !known_model && !body.get("reasoning").is_some_and(Value::is_object) {
        return Ok(());
    }
    let fields = body
        .as_object_mut()
        .ok_or(AiError::InvalidProviderOptions)?
        .entry("include")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or(AiError::InvalidProviderOptions)?;
    if !fields.iter().any(|field| field == ENCRYPTED) {
        fields.push(ENCRYPTED.into());
    }
    Ok(())
}

fn schema_format(output: &StructuredOutput<'_>) -> ResponseFormatJsonSchema {
    ResponseFormatJsonSchema {
        name: "response".into(),
        schema: output.object_schema(),
        strict: Some(output.strict),
        description: None,
    }
}

/// 按上游给出的短等待时间重试一次当前文本请求。
pub(crate) async fn send_with_retry_after<F>(
    endpoint: TextEndpoint,
    send: impl Fn() -> F,
) -> AiResult<Response>
where
    F: std::future::Future<Output = AiResult<Response>>,
{
    let response = send().await?;
    let Some(seconds) = retry_after_seconds(endpoint, &response) else {
        return validate_text_response(endpoint, response);
    };
    drop(response);
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    validate_text_response(endpoint, send().await?)
}

fn retry_after_seconds(endpoint: TextEndpoint, response: &Response) -> Option<u64> {
    let status = response.status().as_u16();
    if status != 429
        && !crate::error::provider_overloaded(status)
        && !(endpoint == TextEndpoint::Messages && status == 529)
    {
        return None;
    }
    let header = response
        .headers()
        .get(http::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    if !header.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let seconds = header.parse().ok().filter(|seconds| *seconds <= 10)?;
    if status == 429 && insufficient_credits(response) {
        return None;
    }
    Some(seconds)
}

fn validate_text_response(endpoint: TextEndpoint, response: Response) -> AiResult<Response> {
    if endpoint == TextEndpoint::Messages && (response.client_error() || response.server_error()) {
        let status = response.status().as_u16();
        if status == 529 {
            return Err(AiError::ProviderOverloaded(status));
        }
        if !matches!(status, 401 | 402 | 429)
            && !crate::error::provider_overloaded(status)
            && let Ok(body) = response.json::<Value>()
            && let Some(message) = body["error"]["message"].as_str()
        {
            let message = message.to_ascii_lowercase();
            if [
                "credit balance",
                "insufficient",
                "quota exceeded",
                "exceeded your current quota",
                "billing",
                "usage limit",
            ]
            .into_iter()
            .any(|pattern| message.contains(pattern))
            {
                return Err(AiError::ProviderInsufficientCredits(status));
            }
        }
    }
    validate(Ok(response))
}

fn insufficient_credits(response: &Response) -> bool {
    let Ok(body) = response.json::<Value>() else {
        return false;
    };
    ["code", "type"].into_iter().any(|field| {
        matches!(
            body["error"][field].as_str(),
            Some(
                "insufficient_quota"
                    | "credit_balance_exhausted"
                    | "organization_spend_limit_exceeded"
                    | "project_spend_limit_exceeded"
                    | "organization_usage_limit_exceeded"
            )
        )
    })
}

pub(crate) fn validate(response: client::Result<Response>) -> AiResult<Response> {
    match response {
        Ok(response) if response.successful() => Ok(response),
        Ok(response) => Err(AiError::ProviderStatus(response.status().as_u16())),
        Err(error) => Err(transport_error(error)),
    }
}

pub(crate) fn transport_error(error: client::Error) -> AiError {
    match error {
        client::Error::ConnectionNotEstablished => AiError::ProviderConnectionFailed,
        client::Error::Timeout => AiError::Timeout,
        client::Error::RequestFailed(response) => {
            AiError::ProviderStatus(response.status().as_u16())
        }
        _ => AiError::Transport,
    }
}

struct ParsedText {
    text: String,
    reasoning: String,
    usage: Usage,
    model: Option<String>,
    citations: Vec<UrlCitation>,
    finish_reason: FinishReason,
}

#[derive(Deserialize)]
struct ChatResponse {
    model: Option<String>,
    choices: Vec<ChatChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
    refusal: Option<String>,
}

fn mistral_document_parts(body: &mut Value) {
    let Some(messages) = body["messages"].as_array_mut() else {
        return;
    };
    for message in messages {
        if let Some(parts) = message["content"].as_array_mut() {
            for part in parts.iter_mut().filter(|part| part["type"] == "file") {
                *part = json!({
                    "type": "document_url",
                    "document_url": part["file"]["file_data"].take(),
                    "document_name": part["file"]["filename"].take(),
                });
            }
        }
    }
}

fn inline_groq_schema(body: &mut Value) -> AiResult<()> {
    let Some(format) = body
        .get("response_format")
        .filter(|format| format["type"] == "json_schema")
    else {
        return Ok(());
    };
    let schema = &format["json_schema"]["schema"];
    let instruction =
        format!("Return only a JSON object matching this schema when answering:\n{schema}");
    body.as_object_mut()
        .ok_or(AiError::InvalidSchema)?
        .remove("response_format");
    let messages = body["messages"]
        .as_array_mut()
        .ok_or(AiError::InvalidSchema)?;
    if let Some(system) = messages
        .first_mut()
        .filter(|message| message["role"] == "system")
    {
        let previous = system["content"].as_str().unwrap_or_default();
        system["content"] = format!("{previous}\n\n{instruction}").into();
    } else {
        messages.insert(0, json!({"role":"system","content":instruction}));
    }
    Ok(())
}

fn parse_native_chat(
    response: Response,
    parse_step: fn(Value) -> AiResult<StepResponse>,
) -> AiResult<ParsedText> {
    let result = parse_step(response.json().map_err(|_| AiError::InvalidResponse)?)?;
    Ok(ParsedText {
        text: result.text.ok_or(AiError::InvalidResponse)?,
        reasoning: result.reasoning,
        usage: result.usage,
        model: result.model,
        citations: result.citations,
        finish_reason: result.finish_reason,
    })
}

fn parse_chat(response: Response) -> AiResult<ParsedText> {
    let response: ChatResponse = response.json().map_err(|_| AiError::InvalidResponse)?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or(AiError::InvalidResponse)?;
    let message = choice.message;
    if message.refusal.is_some_and(|refusal| !refusal.is_empty()) {
        return Err(AiError::Refused);
    }
    Ok(ParsedText {
        text: message.content.ok_or(AiError::InvalidResponse)?,
        reasoning: String::new(),
        usage: response.usage.map(Usage::from).unwrap_or_default(),
        model: response.model,
        citations: Vec::new(),
        finish_reason: FinishReason::from_wire(choice.finish_reason.as_deref()),
    })
}

#[derive(Deserialize)]
struct ResponsesResponse {
    status: Option<String>,
    model: Option<String>,
    output: Vec<ResponsesItem>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesItem {
    Message {
        content: Vec<ResponsesContent>,
    },
    Reasoning {
        #[serde(default)]
        summary: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesContent {
    OutputText {
        text: String,
        #[serde(default)]
        annotations: Value,
    },
    Refusal,
    #[serde(other)]
    Other,
}

fn parse_responses(response: Response) -> AiResult<ParsedText> {
    let response: ResponsesResponse = response.json().map_err(|_| AiError::InvalidResponse)?;
    if response
        .status
        .as_deref()
        .is_some_and(|status| status != "completed")
    {
        return Err(AiError::InvalidResponse);
    }
    let mut text = None::<String>;
    let mut citations = Vec::new();
    let mut reasoning = Vec::new();
    for item in response.output {
        match item {
            ResponsesItem::Message { content } => {
                for part in content {
                    match part {
                        ResponsesContent::OutputText {
                            text: part,
                            annotations,
                        } => {
                            text.get_or_insert_default().push_str(&part);
                            citations.extend(UrlCitation::from_annotations(&annotations));
                        }
                        ResponsesContent::Refusal => return Err(AiError::Refused),
                        ResponsesContent::Other => {}
                    }
                }
            }
            ResponsesItem::Reasoning { summary } => reasoning.extend(public_summary_text(&summary)),
            ResponsesItem::Other => {}
        }
    }
    Ok(ParsedText {
        text: text.ok_or(AiError::InvalidResponse)?,
        reasoning: reasoning.join("\n\n"),
        usage: response.usage.map(Usage::from).unwrap_or_default(),
        model: response.model,
        citations,
        finish_reason: FinishReason::from_wire(response.status.as_deref()),
    })
}

pub(crate) fn public_summary_text(summary: &Value) -> Option<String> {
    let text: String = summary
        .as_array()?
        .iter()
        .filter(|part| part["type"] == "summary_text")
        .filter_map(|part| part["text"].as_str())
        .collect();
    (!text.trim().is_empty()).then_some(text)
}
