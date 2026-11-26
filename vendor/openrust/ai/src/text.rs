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
        client::{self, Factory, PendingRequest, Response},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AiError, AiResult, ModelCatalog, StreamableAgentResponse, models::ListedModel};

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
}

#[derive(Clone, Copy)]
pub(crate) enum TextProtocol {
    ChatCompletions,
    Responses,
}

#[derive(Deserialize)]
struct Configuration {
    name: String,
    url: String,
    #[serde(default)]
    key_env: Option<String>,
    #[serde(default)]
    models: Models,
    #[serde(default = "default_store")]
    store: bool,
}

fn default_store() -> bool {
    true
}

#[derive(Default, Deserialize)]
struct Models {
    #[serde(default)]
    text: TextModels,
}

#[derive(Default, Deserialize)]
struct TextModels {
    default: Option<String>,
    #[serde(default)]
    ids: Vec<String>,
}

pub(crate) struct OpenAiTextProvider {
    protocol: TextProtocol,
    config: Configuration,
    environment: Arc<Environment>,
    http: Arc<Factory>,
}

impl OpenAiTextProvider {
    pub(crate) fn new(
        protocol: TextProtocol,
        config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> AiResult<Self> {
        let config: Configuration = serde_json::from_value(config)
            .map_err(|_| AiError::InvalidConfiguration("providers.<name>"))?;
        let url = url::Url::parse(&config.url)
            .map_err(|_| AiError::InvalidConfiguration("providers.<name>.url"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AiError::InvalidConfiguration("providers.<name>.url"));
        }
        Ok(Self {
            protocol,
            config,
            environment,
            http,
        })
    }

    fn request(&self, timeout: Option<Duration>) -> AiResult<PendingRequest> {
        let mut request = self
            .http
            .pending_request()
            .base_url(&self.config.url)
            .without_redirecting()
            .redact_payload()
            .timeout(timeout.unwrap_or(Duration::from_secs(60)));
        if let Some(key) = &self.config.key_env {
            let token = self
                .environment
                .get_utf8(key)
                .map_err(|_| AiError::InvalidConfiguration("providers.<name>.key_env"))?
                .filter(|token| !token.trim().is_empty())
                .ok_or(AiError::MissingCredential)?;
            request = request.with_token(token);
        }
        Ok(request)
    }

    async fn list_models(&self) -> AiResult<Vec<ListedModel>> {
        #[derive(Deserialize)]
        struct ModelResponse {
            data: Vec<ListedModel>,
        }

        let response = validate(self.request(None)?.get("models").await)?;
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
            store: Some(self.config.store),
            stream: stream.then_some(true),
            ..Default::default()
        }
    }

    async fn send_text(&self, prompt: &TextPrompt<'_>, stream: bool) -> AiResult<Response> {
        let mut request = self.request(prompt.timeout)?;
        if stream {
            request = request.stream();
        }
        let response = match self.protocol {
            TextProtocol::ChatCompletions => {
                request
                    .post("chat/completions", &Self::chat_body(prompt, stream))
                    .await
            }
            TextProtocol::Responses => {
                request
                    .post("responses", &self.responses_body(prompt, stream))
                    .await
            }
        };
        validate(response)
    }
}

impl TextProvider for OpenAiTextProvider {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn default_text_model(&self) -> AiResult<&str> {
        self.config
            .models
            .text
            .default
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async {
            let configured = &self.config.models.text;
            ModelCatalog::merge(
                self.list_models().await,
                configured.default.iter().chain(&configured.ids).cloned(),
            )
        })
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async move {
            let response = self.send_text(&prompt, false).await?;
            let result = match self.protocol {
                TextProtocol::ChatCompletions => parse_chat(response)?,
                TextProtocol::Responses => parse_responses(response)?,
            };
            Ok(AgentResponse {
                text: result.text,
                usage: result.usage,
                provider: self.config.name.clone(),
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
            if !response
                .header("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|content_type| {
                    content_type.split(';').next() == Some("text/event-stream")
                })
            {
                return Err(AiError::InvalidResponse);
            }
            Ok(StreamableAgentResponse::from_upstream(
                self.protocol,
                response.into_stream().map_err(transport_error)?,
                self.config.name.clone(),
                prompt.model.to_owned(),
            ))
        })
    }
}

fn validate(response: client::Result<Response>) -> AiResult<Response> {
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
