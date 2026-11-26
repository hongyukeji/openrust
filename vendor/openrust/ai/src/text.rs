use std::{sync::Arc, time::Duration};

use async_openai::types::{
    chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionStreamOptions,
        CreateChatCompletionRequest,
    },
    responses::{CreateResponse, InputParam},
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
    AiError, AiResult, EmbeddingProvider, ModelCatalog, StreamableAgentResponse,
    connection::{Authorization, Connection},
    embeddings::OpenAiEmbeddingProvider,
    models::ListedModel,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentResponse {
    pub text: String,
    pub usage: Usage,
    pub provider: String,
    pub model: String,
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

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async {
            ModelCatalog::merge(Err(AiError::ModelsNotSupported), std::iter::empty())
        })
    }

    fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
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
            Self::Messages => "messages",
            Self::CountMessageTokens => "messages/count_tokens",
            Self::Embeddings => "embeddings",
            Self::GeminiGenerateContent => "generateContent",
            Self::GeminiStreamGenerateContent => "streamGenerateContent",
            Self::GeminiCountTokens => "countTokens",
        }
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

    fn chat_body(prompt: &TextPrompt<'_>, stream: bool) -> CreateChatCompletionRequest {
        let mut messages = Vec::new();
        if !prompt.instructions.is_empty() {
            messages.push(ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage {
                    content: ChatCompletionRequestSystemMessageContent::Text(
                        prompt.instructions.into(),
                    ),
                    name: None,
                },
            ));
        }
        messages.push(ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Text(prompt.prompt.into()),
                name: None,
            },
        ));
        CreateChatCompletionRequest {
            model: prompt.model.into(),
            messages,
            stream: stream.then_some(true),
            stream_options: stream.then_some(ChatCompletionStreamOptions {
                include_usage: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn responses_body(&self, prompt: &TextPrompt<'_>, stream: bool) -> CreateResponse {
        CreateResponse {
            instructions: (!prompt.instructions.is_empty()).then(|| prompt.instructions.into()),
            input: InputParam::Text(prompt.prompt.into()),
            model: Some(prompt.model.into()),
            store: Some(self.connection.store()),
            stream: stream.then_some(true),
            ..Default::default()
        }
    }

    async fn send_text(&self, prompt: &TextPrompt<'_>, stream: bool) -> AiResult<Response> {
        let endpoint = self.protocol.endpoint();
        let response = match self.protocol {
            OpenAiProtocol::ChatCompletions => {
                self.connection
                    .send(
                        endpoint,
                        &Self::chat_body(prompt, stream),
                        stream,
                        prompt.timeout,
                    )
                    .await?
            }
            OpenAiProtocol::Responses => {
                self.connection
                    .send(
                        endpoint,
                        &self.responses_body(prompt, stream),
                        stream,
                        prompt.timeout,
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

    fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        self.supports_endpoint(TextEndpoint::Embeddings).then(|| {
            Arc::new(OpenAiEmbeddingProvider(self.connection.clone())) as Arc<dyn EmbeddingProvider>
        })
    }

    fn supports_endpoint(&self, endpoint: TextEndpoint) -> bool {
        match endpoint {
            TextEndpoint::Embeddings => {
                matches!(self.protocol, OpenAiProtocol::Responses)
                    || self.connection.embeddings_configured()
            }
            TextEndpoint::ChatCompletions => true,
            _ => self.protocol.endpoint() == endpoint,
        }
    }

    fn relay(&self, endpoint: TextEndpoint, body: Value) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async move {
            if !self.supports_endpoint(endpoint) {
                return Err(AiError::EndpointNotSupported);
            }
            let stream = endpoint != TextEndpoint::Embeddings
                && body.get("stream").and_then(Value::as_bool) == Some(true);
            self.connection.send(endpoint, &body, stream, None).await
        })
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>> {
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
                model: result.model.unwrap_or_else(|| prompt.model.into()),
            })
        })
    }
    fn stream_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
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
                prompt.model.to_owned(),
            ))
        })
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
