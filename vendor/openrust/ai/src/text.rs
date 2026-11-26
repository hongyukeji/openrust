use std::{sync::Arc, time::Duration};

use async_openai::types::{
    chat::{
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionStreamOptions,
        CreateChatCompletionRequest, ResponseFormat, ResponseFormatJsonSchema,
    },
    responses::{
        CreateResponse, EasyInputContent, EasyInputMessage, InputItem, InputParam,
        ResponseTextParam, Role, TextResponseFormatConfiguration,
    },
};
use openrust::{
    config::Environment,
    http::{
        HttpFuture,
        client::{self, Factory, Response},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AgentPrompt, AiError, AiResult, EmbeddingProvider, MessageRole, ModelCatalog,
    RerankingProvider, StepEventStream, StepResponse, StepTextGateway, StreamableAgentResponse,
    StructuredOutput, ToolCall, ToolDefinition, ToolResult, ToolStep,
    audio::{AudioProvider, OpenAiAudioProvider},
    classification::{
        ClassificationProvider, DecisionsProtocol, HttpDecisionsProvider,
        OpenAiClassificationProvider,
    },
    connection::{Authorization, Connection},
    embeddings::OpenAiEmbeddingProvider,
    image::{ImageProvider, OpenAiImageProvider},
    models::ListedModel,
    reranking::{HttpRerankingProvider, RerankingProtocol},
    transcription::{OpenAiTranscriptionProvider, TranscriptionProvider},
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

impl Usage {
    pub(crate) fn combine(mut self, previous: &Self) -> Self {
        self.prompt_tokens = previous
            .prompt_tokens
            .zip(self.prompt_tokens)
            .map(|(previous, current)| previous.saturating_add(current));
        self.completion_tokens = previous
            .completion_tokens
            .zip(self.completion_tokens)
            .map(|(previous, current)| previous.saturating_add(current));
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentResponse {
    pub text: String,
    pub usage: Usage,
    pub provider: String,
    pub model: String,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
}

pub struct TextPrompt<'a> {
    pub instructions: &'a str,
    pub prompt: &'a str,
    pub model: &'a str,
    pub timeout: Option<Duration>,
}

pub trait TextProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_text_model(&self) -> AiResult<&str>;
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEndpoint {
    ChatCompletions,
    Responses,
    CountResponseTokens,
    Messages,
    CountMessageTokens,
    Embeddings,
    GeminiGenerateContent,
    GeminiStreamGenerateContent,
    GeminiCountTokens,
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
            Self::GeminiGenerateContent => "generateContent",
            Self::GeminiStreamGenerateContent => "streamGenerateContent",
            Self::GeminiCountTokens => "countTokens",
        }
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
    Responses,
}

impl OpenAiProtocol {
    fn endpoint(self) -> TextEndpoint {
        match self {
            Self::ChatCompletions => TextEndpoint::ChatCompletions,
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
        Ok(Self {
            protocol,
            connection: Arc::new(Connection::new(
                config,
                environment,
                http,
                Authorization::Bearer,
            )?),
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

    fn chat_body(prompt: &AgentPrompt<'_>, stream: bool) -> CreateChatCompletionRequest {
        let user = |text: &str| {
            ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Text(text.into()),
                name: None,
            })
        };
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
        messages.extend(prompt.messages.iter().map(|message| match message.role {
            MessageRole::User => user(&message.content),
            MessageRole::Assistant => {
                ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                    content: Some(ChatCompletionRequestAssistantMessageContent::Text(
                        message.content.clone(),
                    )),
                    ..Default::default()
                })
            }
        }));
        messages.push(user(prompt.text.prompt));
        CreateChatCompletionRequest {
            model: prompt.text.model.into(),
            messages,
            response_format: prompt.structured_output.as_ref().map(|output| {
                ResponseFormat::JsonSchema {
                    json_schema: schema_format(output),
                }
            }),
            stream: stream.then_some(true),
            stream_options: stream.then_some(ChatCompletionStreamOptions {
                include_usage: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn responses_body(&self, prompt: &AgentPrompt<'_>, stream: bool) -> CreateResponse {
        let input = if prompt.messages.is_empty() {
            InputParam::Text(prompt.text.prompt.into())
        } else {
            let message = |role, content: &str| {
                InputItem::EasyMessage(EasyInputMessage {
                    r#type: Default::default(),
                    role,
                    content: EasyInputContent::Text(content.into()),
                    phase: None,
                })
            };
            InputParam::Items(
                prompt
                    .messages
                    .iter()
                    .map(|entry| {
                        let role = match entry.role {
                            MessageRole::User => Role::User,
                            MessageRole::Assistant => Role::Assistant,
                        };
                        message(role, &entry.content)
                    })
                    .chain(std::iter::once(message(Role::User, prompt.text.prompt)))
                    .collect(),
            )
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
            store: Some(self.connection.store()),
            stream: stream.then_some(true),
            ..Default::default()
        }
    }

    async fn send_text(&self, prompt: &AgentPrompt<'_>, stream: bool) -> AiResult<Response> {
        let endpoint = self.protocol.endpoint();
        let response = match self.protocol {
            OpenAiProtocol::ChatCompletions => {
                self.connection
                    .send(
                        endpoint,
                        &Self::chat_body(prompt, stream),
                        stream,
                        prompt.text.timeout,
                    )
                    .await?
            }
            OpenAiProtocol::Responses => {
                self.connection
                    .send(
                        endpoint,
                        &self.responses_body(prompt, stream),
                        stream,
                        prompt.text.timeout,
                    )
                    .await?
            }
        };
        validate(Ok(response))
    }
}

impl TextProvider for OpenAiTextProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        self.connection.default_text_model()
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async { self.connection.catalog(self.list_models().await) })
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        Some(self)
    }

    fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        self.supports_endpoint(TextEndpoint::Embeddings).then(|| {
            Arc::new(OpenAiEmbeddingProvider(self.connection.clone())) as Arc<dyn EmbeddingProvider>
        })
    }

    fn audio_provider(&self) -> Option<Arc<dyn AudioProvider>> {
        (matches!(self.protocol, OpenAiProtocol::Responses) || self.connection.audio_configured())
            .then(|| {
                Arc::new(OpenAiAudioProvider(self.connection.clone())) as Arc<dyn AudioProvider>
            })
    }

    fn transcription_provider(&self) -> Option<Arc<dyn TranscriptionProvider>> {
        (matches!(self.protocol, OpenAiProtocol::Responses)
            || self.connection.transcription_configured())
        .then(|| {
            Arc::new(OpenAiTranscriptionProvider(self.connection.clone()))
                as Arc<dyn TranscriptionProvider>
        })
    }

    fn image_provider(&self) -> Option<Arc<dyn ImageProvider>> {
        (matches!(self.protocol, OpenAiProtocol::Responses) || self.connection.images_configured())
            .then(|| {
                Arc::new(OpenAiImageProvider(self.connection.clone())) as Arc<dyn ImageProvider>
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
                matches!(self.protocol, OpenAiProtocol::Responses)
                    || self.connection.embeddings_configured()
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
                OpenAiProtocol::Responses => parse_responses(response)?,
            };
            Ok(AgentResponse {
                text: result.text,
                usage: result.usage,
                provider: self.name().into(),
                model: result.model.unwrap_or_else(|| prompt.text.model.into()),
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
            })
        })
    }
    fn stream<'a>(
        &'a self,
        prompt: AgentPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        Box::pin(async move {
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
        match self.protocol {
            OpenAiProtocol::ChatCompletions => {
                let body = serde_json::to_value(Self::chat_body(prompt, stream))
                    .map_err(|_| AiError::InvalidSchema)?;
                crate::tool_protocol::chat_request(body, tools, history)
            }
            OpenAiProtocol::Responses => {
                let body = serde_json::to_value(self.responses_body(prompt, stream))
                    .map_err(|_| AiError::InvalidSchema)?;
                crate::tool_protocol::responses_request(body, tools, history)
            }
        }
    }
}

impl StepTextGateway for OpenAiTextProvider {
    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        Box::pin(async move {
            let payload = self.tool_payload(prompt, tools, history, false)?;
            let response = validate(Ok(self
                .connection
                .send(
                    self.protocol.endpoint(),
                    &payload,
                    false,
                    prompt.text.timeout,
                )
                .await?))?;
            let response = response
                .json::<Value>()
                .map_err(|_| AiError::InvalidResponse)?;
            match self.protocol {
                OpenAiProtocol::ChatCompletions => crate::tool_protocol::chat_step(response),
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
            let response = validate(Ok(self
                .connection
                .send(
                    self.protocol.endpoint(),
                    &payload,
                    true,
                    prompt.text.timeout,
                )
                .await?))?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            let stream = response.into_stream().map_err(transport_error)?;
            Ok(crate::tool_stream_protocol::openai(self.protocol, stream))
        })
    }
}

fn schema_format(output: &StructuredOutput<'_>) -> ResponseFormatJsonSchema {
    ResponseFormatJsonSchema {
        name: "response".into(),
        schema: output.schema.clone(),
        strict: Some(output.strict),
        description: None,
    }
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
        client::Error::Timeout => AiError::Timeout,
        client::Error::RequestFailed(response) => {
            AiError::ProviderStatus(response.status().as_u16())
        }
        _ => AiError::Transport,
    }
}

struct ParsedText {
    text: String,
    usage: Usage,
    model: Option<String>,
}

#[derive(Deserialize)]
struct ChatResponse {
    model: Option<String>,
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
    refusal: Option<String>,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

fn parse_chat(response: Response) -> AiResult<ParsedText> {
    let response: ChatResponse = response.json().map_err(|_| AiError::InvalidResponse)?;
    let message = response
        .choices
        .into_iter()
        .next()
        .ok_or(AiError::InvalidResponse)?
        .message;
    if message.refusal.is_some_and(|refusal| !refusal.is_empty()) {
        return Err(AiError::Refused);
    }
    Ok(ParsedText {
        text: message.content.ok_or(AiError::InvalidResponse)?,
        usage: response
            .usage
            .map(|usage| Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
            })
            .unwrap_or_default(),
        model: response.model,
    })
}

#[derive(Deserialize)]
struct ResponsesResponse {
    status: Option<String>,
    model: Option<String>,
    output: Vec<ResponsesItem>,
    usage: Option<ResponsesUsage>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesItem {
    Message {
        content: Vec<ResponsesContent>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesContent {
    OutputText {
        text: String,
    },
    Refusal,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct ResponsesUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
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
    for item in response.output {
        if let ResponsesItem::Message { content } = item {
            for part in content {
                match part {
                    ResponsesContent::OutputText { text: part } => {
                        text.get_or_insert_default().push_str(&part)
                    }
                    ResponsesContent::Refusal => return Err(AiError::Refused),
                    ResponsesContent::Other => {}
                }
            }
        }
    }
    Ok(ParsedText {
        text: text.ok_or(AiError::InvalidResponse)?,
        usage: response
            .usage
            .map(|usage| Usage {
                prompt_tokens: usage.input_tokens,
                completion_tokens: usage.output_tokens,
            })
            .unwrap_or_default(),
        model: response.model,
    })
}
