use std::{path::Path, sync::Arc, time::Duration};

use aws_config::{BehaviorVersion, sts::AssumeRoleProvider};
use aws_sdk_bedrockruntime::{
    Client,
    config::{Credentials, Region, Token, retry::RetryConfig, timeout::TimeoutConfig},
    error::{ProvideErrorMetadata, SdkError},
    operation::converse::ConverseError,
    primitives::Blob,
    types::{
        ContentBlock, ConversationRole, DocumentBlock, DocumentFormat, DocumentSource, ImageBlock,
        ImageFormat, ImageSource, InferenceConfiguration, Message as BedrockMessage,
        ReasoningContentBlock, SystemContentBlock, TokenUsage,
    },
};
use aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID;
use openrust::{
    config::Environment,
    http::{HttpFuture, client::Factory},
};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::{
    AgentPrompt, AgentResponse, AiError, AiResult, Attachment, FinishReason, MessageRole,
    ModelCatalog, TextModelTier, TextPrompt, TextProvider, Usage,
    connection::{Authorization, Connection},
};

#[derive(Deserialize)]
struct Settings {
    region: Option<String>,
    url: Option<String>,
    key_env: Option<String>,
    #[serde(default = "default_chain")]
    use_default_credential_provider: bool,
    assume_role: Option<AssumeRole>,
}

fn default_chain() -> bool {
    true
}

#[derive(Deserialize)]
struct AssumeRole {
    arn: String,
    session_name: Option<String>,
    duration_seconds: Option<u64>,
    external_id: Option<String>,
}

pub(crate) struct BedrockProvider {
    connection: Connection,
    settings: Settings,
    environment: Arc<Environment>,
    client: OnceCell<Client>,
}

impl BedrockProvider {
    pub(crate) fn new(
        mut config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> AiResult<Self> {
        let settings: Settings = serde_json::from_value(config.clone())
            .map_err(|_| AiError::InvalidConfiguration("providers.<name>"))?;
        if settings.region.as_ref().is_some_and(|region| {
            region.is_empty()
                || !region
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        }) {
            return Err(AiError::InvalidConfiguration("providers.<name>.region"));
        }
        if settings.assume_role.as_ref().is_some_and(|role| {
            role.arn.trim().is_empty()
                || role
                    .duration_seconds
                    .is_some_and(|seconds| !(900..=43200).contains(&seconds))
        }) {
            return Err(AiError::InvalidConfiguration(
                "providers.<name>.assume_role",
            ));
        }
        if settings.url.is_none() {
            config["url"] = "https://bedrock-runtime.amazonaws.com".into();
        }
        let connection = Connection::new(config, environment.clone(), http, Authorization::Bearer)?;
        Ok(Self {
            connection,
            settings,
            environment,
            client: OnceCell::new(),
        })
    }

    fn env(&self, name: &str) -> AiResult<Option<String>> {
        self.environment
            .get_utf8(name)
            .map(|value| {
                value
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
            })
            .map_err(|_| AiError::InvalidConfiguration("providers.<name>.credentials"))
    }

    async fn client(&self) -> AiResult<&Client> {
        self.client
            .get_or_try_init(|| async {
                let region = Region::new(
                    self.settings
                        .region
                        .clone()
                        .unwrap_or_else(|| "us-east-1".into()),
                );
                let retry = RetryConfig::standard().with_max_attempts(1);
                let key = match self.settings.key_env.as_deref() {
                    Some(name) => self.env(name)?,
                    None => self.env("AWS_BEARER_TOKEN_BEDROCK")?,
                };
                let mut config = if let Some(key) = key {
                    aws_sdk_bedrockruntime::config::Builder::new()
                        .behavior_version(BehaviorVersion::latest())
                        .region(region)
                        .bearer_token(Token::new(key, None))
                        .auth_scheme_preference([HTTP_BEARER_AUTH_SCHEME_ID])
                } else {
                    let mut loader = aws_config::defaults(BehaviorVersion::latest()).region(region);
                    match (
                        self.env("AWS_ACCESS_KEY_ID")?,
                        self.env("AWS_SECRET_ACCESS_KEY")?,
                    ) {
                        (Some(id), Some(secret)) => {
                            loader = loader.credentials_provider(Credentials::new(
                                id,
                                secret,
                                self.env("AWS_SESSION_TOKEN")?,
                                None,
                                "openrust",
                            ));
                        }
                        (None, None) if self.settings.use_default_credential_provider => {}
                        _ => return Err(AiError::MissingCredential),
                    }
                    let shared = loader.load().await;
                    let mut config = aws_sdk_bedrockruntime::config::Builder::from(&shared);
                    if let Some(role) = &self.settings.assume_role {
                        let mut assume =
                            AssumeRoleProvider::builder(role.arn.clone()).configure(&shared);
                        if let Some(name) = &role.session_name {
                            assume = assume.session_name(name);
                        }
                        if let Some(seconds) = role.duration_seconds {
                            assume = assume.session_length(Duration::from_secs(seconds));
                        }
                        if let Some(id) = &role.external_id {
                            assume = assume.external_id(id);
                        }
                        config = config.credentials_provider(assume.build().await);
                    }
                    config
                };
                if let Some(endpoint) = &self.settings.url {
                    config = config.endpoint_url(endpoint);
                }
                Ok(Client::from_conf(config.retry_config(retry).build()))
            })
            .await
    }
}

impl TextProvider for BedrockProvider {
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
        Box::pin(async { self.connection.catalog(Err(AiError::ModelsNotSupported)) })
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>> {
        self.generate(prompt.into())
    }

    fn generate<'a>(&'a self, prompt: AgentPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async move {
            if prompt.structured_output.is_some() {
                return Err(AiError::StructuredOutputNotSupported);
            }
            if prompt
                .provider_options
                .is_some_and(|options| !options.is_empty())
            {
                return Err(AiError::ProviderOptionsNotSupported);
            }
            let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
            for message in prompt.messages {
                let role = match message.role {
                    MessageRole::User => ConversationRole::User,
                    MessageRole::Assistant if message.tool_calls.is_empty() => {
                        ConversationRole::Assistant
                    }
                    _ => return Err(AiError::ToolsNotSupported),
                };
                messages.push(message_content(
                    role,
                    &message.content,
                    &message.attachments,
                )?);
            }
            messages.push(message_content(
                ConversationRole::User,
                prompt.text.prompt,
                prompt.attachments,
            )?);
            let options = prompt.options;
            let max_tokens = options
                .max_tokens
                .map(i32::try_from)
                .transpose()
                .map_err(|_| AiError::InvalidGenerationOptions)?;
            let inference = InferenceConfiguration::builder()
                .set_max_tokens(max_tokens)
                .set_temperature(options.temperature)
                .set_top_p(options.top_p)
                .build();
            let client = self.client().await?;
            let mut request = client
                .converse()
                .model_id(prompt.text.model)
                .set_messages(Some(messages));
            if options != Default::default() {
                request = request.inference_config(inference);
            }
            if !prompt.text.instructions.is_empty() {
                request = request.system(SystemContentBlock::Text(prompt.text.instructions.into()));
            }
            let timeout = TimeoutConfig::builder()
                .operation_timeout(prompt.text.timeout.unwrap_or(Duration::from_secs(60)))
                .build();
            let response = request
                .customize()
                .config_override(
                    aws_sdk_bedrockruntime::config::Builder::new().timeout_config(timeout),
                )
                .send()
                .await
                .map_err(converse_error)?;
            let message = response
                .output()
                .and_then(|output| output.as_message().ok())
                .ok_or(AiError::InvalidResponse)?;
            let mut text = String::new();
            let mut reasoning = Vec::new();
            for item in message.content() {
                match item {
                    ContentBlock::Text(part) => text.push_str(part),
                    ContentBlock::ReasoningContent(ReasoningContentBlock::ReasoningText(part)) => {
                        reasoning.push(part.text());
                    }
                    _ => {}
                }
            }
            if text.is_empty() {
                return Err(AiError::InvalidResponse);
            }
            let mut result = AgentResponse::new(
                text,
                usage(response.usage()),
                self.name(),
                prompt.text.model,
            );
            result.reasoning = reasoning.join("\n\n");
            Ok(result.with_step(finish_reason(response.stop_reason().as_str())))
        })
    }
}

fn message_content(
    role: ConversationRole,
    text: &str,
    attachments: &[Attachment],
) -> AiResult<BedrockMessage> {
    if role != ConversationRole::User && !attachments.is_empty() {
        return Err(AiError::AttachmentsNotSupported);
    }
    let mut blocks = Vec::with_capacity(1 + attachments.len());
    if !text.trim().is_empty() {
        blocks.push(ContentBlock::Text(text.into()));
    }
    for attachment in attachments {
        blocks.push(match attachment {
            Attachment::Image(image) => {
                let format = match image.mime() {
                    "image/jpeg" => ImageFormat::Jpeg,
                    "image/png" => ImageFormat::Png,
                    "image/webp" => ImageFormat::Webp,
                    _ => return Err(AiError::InvalidImage),
                };
                ContentBlock::Image(
                    ImageBlock::builder()
                        .format(format)
                        .source(ImageSource::Bytes(Blob::new(image.content())))
                        .build()
                        .map_err(|_| AiError::InvalidImage)?,
                )
            }
            Attachment::Document(document) => {
                if text.trim().is_empty() {
                    return Err(AiError::InvalidPrompt);
                }
                let format = match document.mime() {
                    "application/pdf" => DocumentFormat::Pdf,
                    "text/plain" => DocumentFormat::Txt,
                    "text/markdown" => DocumentFormat::Md,
                    _ => return Err(AiError::InvalidDocument),
                };
                ContentBlock::Document(
                    DocumentBlock::builder()
                        .format(format)
                        .name(document_name(document.filename()))
                        .source(DocumentSource::Bytes(Blob::new(document.content())))
                        .build()
                        .map_err(|_| AiError::InvalidDocument)?,
                )
            }
        });
    }
    if blocks.is_empty() {
        return Err(AiError::InvalidPrompt);
    }
    BedrockMessage::builder()
        .role(role)
        .set_content(Some(blocks))
        .build()
        .map_err(|_| AiError::InvalidPrompt)
}

fn document_name(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(filename);
    let name = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut safe = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric()
            || matches!(character, '-' | '(' | ')' | '[' | ']' | ' ')
        {
            safe.push(character);
        } else if !safe.ends_with('-') {
            safe.push('-');
        }
    }
    if safe.is_empty() {
        "document".into()
    } else {
        safe
    }
}

fn usage(usage: Option<&TokenUsage>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    Usage {
        prompt_tokens: usage.input_tokens().try_into().ok(),
        completion_tokens: usage.output_tokens().try_into().ok(),
        cache_read_input_tokens: usage
            .cache_read_input_tokens()
            .and_then(|value| value.try_into().ok()),
        cache_write_input_tokens: usage
            .cache_write_input_tokens()
            .and_then(|value| value.try_into().ok()),
        ..Usage::default()
    }
    .include_cached_input()
}

fn finish_reason(reason: &str) -> FinishReason {
    match reason {
        "guardrail_intervened" | "content_filtered" => FinishReason::ContentFilter,
        _ => FinishReason::from_wire(Some(reason)),
    }
}

fn converse_error(error: SdkError<ConverseError>) -> AiError {
    if matches!(error, SdkError::TimeoutError(_)) {
        return AiError::Timeout;
    }
    if let Some(raw) = error.raw_response() {
        let status = raw.status().as_u16();
        return match error
            .as_service_error()
            .and_then(ProvideErrorMetadata::code)
        {
            Some(
                "ServiceUnavailableException"
                | "ModelNotReadyException"
                | "ModelTimeoutException"
                | "ModelStreamErrorException"
                | "InternalServerException",
            ) => AiError::ProviderOverloaded(status),
            Some("ServiceQuotaExceededException") => AiError::ProviderInsufficientCredits(status),
            _ => AiError::ProviderStatus(status),
        };
    }
    match error {
        SdkError::DispatchFailure(failure) if failure.is_timeout() => AiError::Timeout,
        _ => AiError::Transport,
    }
}
