mod embeddings;
mod image;
mod invoke;
mod reranking;
mod structured;
mod tools;

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use aws_config::{BehaviorVersion, sts::AssumeRoleProvider};
use aws_sdk_bedrockruntime::{
    Client,
    config::{Credentials, Region, Token, retry::RetryConfig, timeout::TimeoutConfig},
    error::{ProvideErrorMetadata, SdkError},
    operation::converse_stream::ConverseStreamOutput as StreamOutput,
    primitives::Blob,
    types::{
        ContentBlock, ContentBlockDelta, ConversationRole, ConverseStreamOutput, DocumentBlock,
        DocumentFormat, DocumentSource, ImageBlock, ImageFormat, ImageSource,
        InferenceConfiguration, Message as BedrockMessage, ReasoningContentBlock,
        ReasoningContentBlockDelta, SystemContentBlock, TokenUsage, ToolConfiguration,
        error::ConverseStreamOutputError,
    },
};
use aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID;
use openrust::support::Str;
use openrust::{
    config::Environment,
    http::{HttpFuture, client::Factory},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::{
    AgentPrompt, AgentResponse, AiError, AiResult, Attachment, FinishReason, MessageRole,
    ModelCatalog, ReasoningPart, StepEvent, StepEventStream, StepResponse, StepTextGateway,
    StreamableAgentResponse, TextModelTier, TextPrompt, TextProvider, ToolCall, ToolDefinition,
    ToolStep, Usage,
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

#[derive(Deserialize, Serialize)]
struct AssumeRole {
    arn: String,
    session_name: Option<String>,
    duration_seconds: Option<u64>,
    external_id: Option<String>,
}

#[derive(Clone)]
pub(crate) struct BedrockProvider {
    connection: Arc<Connection>,
    settings: Arc<Settings>,
    environment: Arc<Environment>,
    client: Arc<OnceCell<Client>>,
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
            connection: Arc::new(connection),
            settings: Arc::new(settings),
            environment,
            client: Arc::new(OnceCell::new()),
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

    fn bearer_key(&self) -> AiResult<Option<String>> {
        self.env(
            self.settings
                .key_env
                .as_deref()
                .unwrap_or("AWS_BEARER_TOKEN_BEDROCK"),
        )
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
                let key = self.bearer_key()?;
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

    fn embedding_provider(&self) -> Option<Arc<dyn crate::EmbeddingProvider>> {
        Some(Arc::new(self.clone()))
    }

    fn image_provider(&self) -> Option<Arc<dyn crate::ImageProvider>> {
        Some(Arc::new(self.clone()))
    }

    fn reranking_provider(&self) -> Option<Arc<dyn crate::RerankingProvider>> {
        Some(Arc::new(self.clone()))
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>> {
        self.generate(prompt.into())
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        Some(self)
    }

    fn generate<'a>(&'a self, prompt: AgentPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async move {
            let completed = self.generate_text_step(&prompt, &[], &[]).await?;
            let text = completed
                .text
                .filter(|text| !text.is_empty())
                .ok_or(AiError::InvalidResponse)?;
            let mut result =
                AgentResponse::new(text, completed.usage, self.name(), prompt.text.model);
            result.reasoning = completed.reasoning;
            Ok(result.with_step(completed.finish_reason))
        })
    }

    fn stream_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        self.stream(prompt.into())
    }

    fn stream<'a>(
        &'a self,
        prompt: AgentPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        Box::pin(async move {
            let events = self.stream_text_step(&prompt, &[], &[]).await?;
            Ok(crate::tool_stream::single_step_response(
                Arc::new(self.clone()),
                prompt,
                events,
            ))
        })
    }
}

impl StepTextGateway for BedrockProvider {
    fn approval_replay_scope(&self) -> AiResult<Option<String>> {
        let auth = if let Some(token) = self.bearer_key()? {
            serde_json::json!(["bearer", token])
        } else if let (Some(id), Some(secret)) = (
            self.env("AWS_ACCESS_KEY_ID")?,
            self.env("AWS_SECRET_ACCESS_KEY")?,
        ) {
            serde_json::json!([
                "sigv4",
                id,
                secret,
                self.env("AWS_SESSION_TOKEN")?,
                self.settings.assume_role
            ])
        } else {
            // 默认凭证链的调用身份无法在同步恢复检查中确认。
            return Ok(None);
        };
        let scope = serde_json::to_vec(&(
            "bedrock",
            self.name(),
            &self.settings.region,
            &self.settings.url,
            auth,
        ))
        .map_err(|_| AiError::InvalidConfiguration("providers.<name>"))?;
        Ok(Some(format!("{:x}", Sha256::digest(scope))))
    }

    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        definitions: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        self.generate_text_step_with_context(prompt, definitions, history, false)
    }

    fn generate_text_step_with_context<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        definitions: &'a [ToolDefinition],
        history: &'a [ToolStep],
        final_step: bool,
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        Box::pin(async move {
            let scope = self.approval_replay_scope()?;
            let input =
                ConverseInput::prepare(prompt, definitions, history, scope.as_deref(), final_step)?;
            let allow_tools = input.tool_config.is_some();
            let request = self
                .client()
                .await?
                .converse()
                .model_id(prompt.text.model)
                .set_messages(Some(input.messages))
                .set_system(input.system)
                .set_inference_config(input.inference)
                .set_tool_config(input.tool_config);
            let response = request
                .customize()
                .config_override(input.timeout)
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
            let blocks = tools::read_output(message.content())?;
            let (calls, replay) = tools::output_state(&blocks)?;
            if !calls.is_empty() && !allow_tools {
                return Err(AiError::ToolsNotSupported);
            }
            let reason = finish_reason(response.stop_reason().as_str());
            validate_tool_stop(reason, &calls)?;
            input.mode.finish(StepResponse {
                model: None,
                text: (!text.is_empty()).then_some(text),
                reasoning: reasoning.join("\n\n"),
                finish_reason: reason,
                usage: usage(response.usage()),
                citations: Vec::new(),
                tool_calls: calls,
                provider_content: replay,
            })
        })
    }

    fn stream_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        definitions: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepEventStream>> {
        self.stream_text_step_with_context(prompt, definitions, history, false)
    }

    fn stream_text_step_with_context<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        definitions: &'a [ToolDefinition],
        history: &'a [ToolStep],
        final_step: bool,
    ) -> HttpFuture<'a, AiResult<StepEventStream>> {
        Box::pin(async move {
            let scope = self.approval_replay_scope()?;
            let input =
                ConverseInput::prepare(prompt, definitions, history, scope.as_deref(), final_step)?;
            let allow_tools = input.tool_config.is_some();
            let client = self.client().await?;
            let deadline = tokio::time::Instant::now()
                .checked_add(input.duration)
                .ok_or(AiError::InvalidGenerationOptions)?;
            let response = client
                .converse_stream()
                .model_id(prompt.text.model)
                .set_messages(Some(input.messages))
                .set_system(input.system)
                .set_inference_config(input.inference)
                .set_tool_config(input.tool_config)
                .customize()
                .config_override(input.timeout)
                .send()
                .await
                .map_err(converse_error)?;
            Ok(stream_events(response, deadline, allow_tools, input.mode))
        })
    }
}

struct ConverseInput {
    messages: Vec<BedrockMessage>,
    system: Option<Vec<SystemContentBlock>>,
    inference: Option<InferenceConfiguration>,
    tool_config: Option<ToolConfiguration>,
    mode: structured::OutputMode,
    duration: Duration,
    timeout: aws_sdk_bedrockruntime::config::Builder,
}

impl ConverseInput {
    fn prepare(
        prompt: &AgentPrompt<'_>,
        definitions: &[ToolDefinition],
        history: &[ToolStep],
        scope: Option<&str>,
        final_step: bool,
    ) -> AiResult<Self> {
        if prompt
            .provider_options
            .is_some_and(|options| !options.is_empty())
        {
            return Err(AiError::ProviderOptionsNotSupported);
        }
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        for message in prompt.messages {
            messages.push(match message.role {
                MessageRole::User => message_content(
                    ConversationRole::User,
                    &message.content,
                    &message.attachments,
                )?,
                MessageRole::Assistant if message.attachments.is_empty() => {
                    let replay =
                        scope
                            .zip(message.provider_replay.as_ref())
                            .and_then(|(scope, replay)| {
                                replay.bedrock_blocks(scope, prompt.text.model)
                            });
                    tools::assistant(&message.content, &message.tool_calls, replay)?
                }
                MessageRole::ToolResult if message.attachments.is_empty() => {
                    tools::results(&message.tool_results)?
                }
                _ => return Err(AiError::AttachmentsNotSupported),
            });
        }
        messages.push(message_content(
            ConversationRole::User,
            prompt.text.prompt,
            prompt.attachments,
        )?);
        messages.extend(tools::messages(history)?);
        let output = structured::prepare(prompt, definitions, final_step)?;
        let options = prompt.options;
        let max_tokens = options
            .max_tokens
            .map(i32::try_from)
            .transpose()
            .map_err(|_| AiError::InvalidGenerationOptions)?;
        let inference = (options != Default::default()).then(|| {
            InferenceConfiguration::builder()
                .set_max_tokens(max_tokens)
                .set_temperature(options.temperature)
                .set_top_p(options.top_p)
                .build()
        });
        let mut system = Vec::with_capacity(2);
        if !prompt.text.instructions.is_empty() {
            system.push(SystemContentBlock::Text(prompt.text.instructions.into()));
        }
        if let Some(instruction) = output.instruction {
            system.push(SystemContentBlock::Text(instruction));
        }
        let system = (!system.is_empty()).then_some(system);
        let duration = prompt.text.timeout.unwrap_or(Duration::from_secs(60));
        let timeout = aws_sdk_bedrockruntime::config::Builder::new()
            .timeout_config(TimeoutConfig::builder().operation_timeout(duration).build());
        Ok(Self {
            messages,
            system,
            inference,
            tool_config: output.tool_config,
            mode: output.mode,
            duration,
            timeout,
        })
    }
}

#[derive(Default)]
struct StreamBlock {
    text: String,
    reasoning: String,
    signature: String,
    redacted: Vec<u8>,
    tool: Option<(String, String, String)>,
    closed: bool,
}

fn stream_events(
    mut response: StreamOutput,
    deadline: tokio::time::Instant,
    allow_tools: bool,
    mode: structured::OutputMode,
) -> StepEventStream {
    Box::pin(async_stream::try_stream! {
        let mut started = false;
        let mut stopped = None;
        let mut blocks = BTreeMap::<u64, StreamBlock>::new();
        while let Some(event) = tokio::time::timeout_at(deadline, response.stream.recv())
            .await.map_err(|_| AiError::Timeout)?.map_err(stream_error)?
        {
            match event {
                ConverseStreamOutput::MessageStart(event) if !started && event.role() == &ConversationRole::Assistant => {
                    started = true;
                }
                ConverseStreamOutput::ContentBlockStart(event) if started && stopped.is_none() => {
                    let index = u64::try_from(event.content_block_index()).map_err(|_| AiError::InvalidResponse)?;
                    let tool = event.start().and_then(|start| start.as_tool_use().ok())
                        .ok_or(AiError::InvalidResponse)?;
                    if !allow_tools || blocks.contains_key(&index) {
                        Err(AiError::InvalidResponse)?;
                    }
                    blocks.insert(index, StreamBlock {
                        tool: Some((tool.tool_use_id().into(), tool.name().into(), String::new())),
                        ..StreamBlock::default()
                    });
                }
                ConverseStreamOutput::ContentBlockDelta(event) if started && stopped.is_none() => {
                    let index = u64::try_from(event.content_block_index()).map_err(|_| AiError::InvalidResponse)?;
                    let part = ("bedrock".into(), index);
                    let block = blocks.entry(index).or_default();
                    if block.closed {
                        Err(AiError::InvalidResponse)?;
                    }
                    match event.delta().ok_or(AiError::InvalidResponse)? {
                        ContentBlockDelta::Text(delta) if !delta.is_empty() => {
                            block.text.push_str(delta);
                            if !mode.buffers_text() {
                                yield StepEvent::TextPartDelta { part, delta: delta.clone() };
                            }
                        }
                        ContentBlockDelta::ReasoningContent(ReasoningContentBlockDelta::Text(delta)) if !delta.is_empty() => {
                            block.reasoning.push_str(delta);
                            yield StepEvent::Reasoning(ReasoningPart::Delta { part, delta: delta.clone() });
                        }
                        ContentBlockDelta::ReasoningContent(ReasoningContentBlockDelta::Signature(delta)) => {
                            block.signature.push_str(delta);
                        }
                        ContentBlockDelta::ReasoningContent(ReasoningContentBlockDelta::RedactedContent(delta)) => {
                            block.redacted.extend_from_slice(delta.as_ref());
                        }
                        ContentBlockDelta::ToolUse(delta) => {
                            let (_, _, arguments) = block.tool.as_mut().ok_or(AiError::InvalidResponse)?;
                            arguments.push_str(delta.input());
                        }
                        _ => {}
                    }
                }
                ConverseStreamOutput::ContentBlockStop(event) if started && stopped.is_none() => {
                    let index = u64::try_from(event.content_block_index()).map_err(|_| AiError::InvalidResponse)?;
                    let block = blocks.entry(index).or_default();
                    if block.closed {
                        Err(AiError::InvalidResponse)?;
                    }
                    block.closed = true;
                    if !mode.buffers_text() && !block.text.is_empty() {
                        yield StepEvent::TextPartEnd { part: ("bedrock".into(), index) };
                    }
                    if !block.reasoning.is_empty() {
                        yield StepEvent::Reasoning(ReasoningPart::End { part: ("bedrock".into(), index), text: None });
                    }
                }
                ConverseStreamOutput::MessageStop(event) if started && stopped.is_none() => {
                    if blocks.values().any(|block| !block.closed) {
                        Err(AiError::InvalidResponse)?;
                    }
                    stopped = Some(finish_reason(event.stop_reason().as_str()));
                }
                ConverseStreamOutput::Metadata(event) => {
                    let reason = stopped.ok_or(AiError::InvalidResponse)?;
                    let tokens = event.usage().ok_or(AiError::InvalidResponse)?;
                    let text = blocks.values().map(|block| block.text.as_str()).collect::<String>();
                    let reasoning = blocks.values().map(|block| block.reasoning.as_str())
                        .filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n\n");
                    let replay = blocks.into_values().filter_map(stream_block).collect::<AiResult<Vec<_>>>()?;
                    let (calls, replay) = tools::output_state(&replay)?;
                    validate_tool_stop(reason, &calls)?;
                    let response = mode.finish(StepResponse {
                        model: None,
                        text: (!text.is_empty()).then_some(text),
                        reasoning,
                        finish_reason: reason,
                        usage: usage(Some(tokens)),
                        citations: Vec::new(),
                        tool_calls: calls,
                        provider_content: replay,
                    })?;
                    let reason = response.finish_reason.as_str().into();
                    yield StepEvent::Finished { response: Box::new(response), reason };
                    return;
                }
                event if event.is_unknown() => {}
                _ => Err(AiError::InvalidResponse)?,
            }
        }
        Err(AiError::InvalidResponse)?;
    })
}

fn validate_tool_stop(reason: FinishReason, calls: &[ToolCall]) -> AiResult<()> {
    if (reason == FinishReason::ToolCalls) == calls.is_empty() {
        return Err(AiError::InvalidResponse);
    }
    Ok(())
}

fn stream_block(block: StreamBlock) -> Option<AiResult<tools::ReplayBlock>> {
    use tools::ReplayBlock;
    if let Some((id, name, arguments)) = block.tool {
        let arguments = if arguments.trim().is_empty() {
            Ok(serde_json::Map::new())
        } else {
            serde_json::from_str(&arguments).map_err(|_| AiError::InvalidToolArguments)
        };
        return Some(arguments.map(|arguments| {
            ReplayBlock::Tool(ToolCall {
                id,
                name,
                arguments,
                result_id: None,
            })
        }));
    }
    if !block.signature.is_empty() {
        return Some(Ok(ReplayBlock::SignedReasoning {
            text: block.reasoning,
            signature: block.signature,
        }));
    }
    if !block.redacted.is_empty() {
        return Some(Ok(ReplayBlock::RedactedReasoning(Str::to_base64(
            block.redacted,
        ))));
    }
    (!block.text.is_empty()).then_some(Ok(ReplayBlock::Text(block.text)))
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

fn converse_error<E: ProvideErrorMetadata>(error: SdkError<E>) -> AiError {
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

fn stream_error<R>(error: SdkError<ConverseStreamOutputError, R>) -> AiError {
    if matches!(&error, SdkError::TimeoutError(_))
        || matches!(&error, SdkError::DispatchFailure(failure) if failure.is_timeout())
    {
        return AiError::Timeout;
    }
    match error.as_service_error() {
        Some(ConverseStreamOutputError::ThrottlingException(_)) => AiError::ProviderStatus(429),
        Some(ConverseStreamOutputError::ServiceUnavailableException(_)) => {
            AiError::ProviderOverloaded(503)
        }
        Some(ConverseStreamOutputError::InternalServerException(_)) => {
            AiError::ProviderOverloaded(500)
        }
        Some(ConverseStreamOutputError::ModelStreamErrorException(_)) => {
            AiError::ProviderOverloaded(424)
        }
        Some(ConverseStreamOutputError::ValidationException(_)) => AiError::ProviderStatus(400),
        _ => AiError::InvalidResponse,
    }
}
