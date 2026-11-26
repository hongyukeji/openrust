use std::{collections::HashSet, sync::Arc};

use openrust::{
    config::Environment,
    http::{
        HttpFuture,
        client::{Factory, Response},
    },
    support::Str,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AgentPrompt, AgentResponse, AiError, AiResult, Attachment, ModelCatalog, StepEventStream,
    StepResponse, StepTextGateway, StreamableAgentResponse, TextEndpoint, TextModelTier,
    TextPrompt, TextProvider, ToolCall, ToolChoice, ToolDefinition, ToolStep, Usage,
    connection::{Authorization, Connection},
    files::{FileProvider, WorkspaceFileProtocol, WorkspaceFileProvider},
    models::ListedModel,
    provider_options::apply_provider_options,
    text::{transport_error, validate},
};

pub(crate) struct AnthropicTextProvider(Arc<Connection>);

impl AnthropicTextProvider {
    pub(crate) fn new(
        config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> AiResult<Self> {
        Ok(Self(Arc::new(Connection::new(
            config,
            environment,
            http,
            Authorization::Anthropic,
        )?)))
    }

    fn prompt_body(prompt: &AgentPrompt<'_>) -> AiResult<Value> {
        let mut messages: Vec<_> = prompt
            .messages
            .iter()
            .map(|message| json!({"role":message.role.as_str(), "content":message.content}))
            .collect();
        let content = if prompt.attachments.is_empty() {
            Value::from(prompt.text.prompt)
        } else {
            let mut blocks = Vec::new();
            if !prompt.text.prompt.trim().is_empty() {
                blocks.push(json!({"type":"text","text":prompt.text.prompt}));
            }
            for attachment in prompt.attachments {
                blocks.push(match attachment {
                    Attachment::Image(image) => json!({
                        "type":"image",
                        "source":{"type":"base64","media_type":image.mime(),"data":Str::to_base64(image.content())}
                    }),
                    Attachment::Document(document) if document.is_pdf() => json!({
                        "type":"document","title":document.filename(),
                        "source":{"type":"base64","media_type":document.mime(),"data":Str::to_base64(document.content())}
                    }),
                    Attachment::Document(document) => json!({
                        "type":"document","title":document.filename(),
                        "source":{"type":"text","media_type":"text/plain","data":document.text()?}
                    }),
                });
            }
            Value::Array(blocks)
        };
        messages.push(json!({"role":"user","content":content}));
        let mut body = json!({
            "model": prompt.text.model,
            "max_tokens": prompt.options.max_tokens.unwrap_or(64_000),
            "messages": messages,
        });
        if !prompt.text.instructions.is_empty() {
            body["system"] = prompt.text.instructions.into();
        }
        if supports_sampling(prompt.text.model) {
            if let Some(temperature) = prompt.options.temperature {
                if temperature > 1.0 {
                    return Err(AiError::InvalidGenerationOptions);
                }
                body["temperature"] = temperature.into();
            } else if let Some(top_p) = prompt.options.top_p {
                body["top_p"] = top_p.into();
            }
        }
        if let Some(output) = &prompt.structured_output {
            body["output_config"] =
                json!({"format":{"type":"json_schema","schema":output.object_schema()}});
        }
        apply_provider_options(&mut body, prompt.provider_options, false)?;
        Ok(body)
    }

    async fn send(&self, prompt: &AgentPrompt<'_>, stream: bool) -> AiResult<Response> {
        let mut body = Self::prompt_body(prompt)?;
        if stream {
            body["stream"] = true.into();
        }
        validate(Ok(self
            .0
            .send(TextEndpoint::Messages, &body, stream, prompt.text.timeout)
            .await?))
    }

    async fn list_models(&self) -> AiResult<Vec<ListedModel>> {
        #[derive(Deserialize)]
        struct Page {
            data: Vec<ListedModel>,
            has_more: bool,
            last_id: Option<String>,
        }

        let mut models = Vec::new();
        let mut cursor = None::<String>;
        let mut seen = HashSet::new();
        for _ in 0..100 {
            let mut request = self.0.request(None)?;
            if let Some(cursor) = &cursor {
                request = request.with_query_parameters([("after_id".into(), cursor.clone())]);
            }
            let response = validate(request.get("models").await)?;
            let page: Page = response.json().map_err(|_| AiError::InvalidResponse)?;
            models.extend(page.data);
            if !page.has_more {
                return Ok(models);
            }
            cursor = Some(
                page.last_id
                    .filter(|id| !id.is_empty() && seen.insert(id.clone()))
                    .ok_or(AiError::InvalidResponse)?,
            );
        }
        Err(AiError::InvalidResponse)
    }
}

fn supports_sampling(model: &str) -> bool {
    let Some(model) = model.strip_prefix("claude-") else {
        return false;
    };
    if model.starts_with("3-") {
        return true;
    }
    let mut segments = model.split('-');
    if !matches!(segments.next(), Some("opus" | "sonnet" | "haiku")) || segments.next() != Some("4")
    {
        return false;
    }
    let date = |value: &str| value.len() == 8 && value.bytes().all(|digit| digit.is_ascii_digit());
    match (segments.next(), segments.next(), segments.next()) {
        (None, None, None) => true,
        (Some(value), None, None) => {
            date(value) || matches!(value, "0" | "1" | "2" | "3" | "4" | "5" | "6")
        }
        (Some("0" | "1" | "2" | "3" | "4" | "5" | "6"), Some(value), None) => date(value),
        _ => false,
    }
}

impl TextProvider for AnthropicTextProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        self.0.default_text_model()
    }

    fn text_model(&self, tier: TextModelTier) -> AiResult<&str> {
        self.0.text_model(tier)
    }

    fn file_provider(&self) -> Option<Arc<dyn FileProvider>> {
        Some(Arc::new(WorkspaceFileProvider {
            connection: self.0.clone(),
            protocol: WorkspaceFileProtocol::Anthropic,
        }))
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
            let response: Value = self
                .send(&prompt, false)
                .await?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            if response["stop_reason"] == "refusal" {
                return Err(AiError::Refused);
            }
            let text: String = response["content"]
                .as_array()
                .ok_or(AiError::InvalidResponse)?
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect();
            if text.is_empty() {
                return Err(AiError::InvalidResponse);
            }
            Ok(AgentResponse {
                text,
                usage: Usage::from_anthropic(&response["usage"]).include_cached_input(),
                provider: self.name().into(),
                model: response["model"]
                    .as_str()
                    .unwrap_or(prompt.text.model)
                    .into(),
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
            let response = self.send(&prompt, true).await?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            Ok(StreamableAgentResponse::from_upstream(
                TextEndpoint::Messages,
                response.into_stream().map_err(transport_error)?,
                self.name().into(),
                prompt.text.model.to_owned(),
            ))
        })
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        Some(self)
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async { self.0.catalog(self.list_models().await) })
    }

    fn supports_endpoint(&self, endpoint: TextEndpoint) -> bool {
        matches!(
            endpoint,
            TextEndpoint::Messages | TextEndpoint::CountMessageTokens
        )
    }

    fn relay(&self, endpoint: TextEndpoint, body: Value) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async move {
            if !self.supports_endpoint(endpoint) {
                return Err(AiError::EndpointNotSupported);
            }
            let stream = endpoint == TextEndpoint::Messages && body["stream"] == true;
            self.0.send(endpoint, &body, stream, None).await
        })
    }
}

impl StepTextGateway for AnthropicTextProvider {
    fn approval_replay_scope(&self) -> AiResult<Option<String>> {
        self.0.request_namespace("messages-tool-replay").map(Some)
    }

    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        Box::pin(async move {
            let body = Self::tool_payload(prompt, tools, history)?;

            let response: Value = validate(Ok(self
                .0
                .send(TextEndpoint::Messages, &body, false, prompt.text.timeout)
                .await?))?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            parse_step(response)
        })
    }

    fn stream_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepEventStream>> {
        Box::pin(async move {
            let mut body = Self::tool_payload(prompt, tools, history)?;
            body["stream"] = true.into();
            let response = validate(Ok(self
                .0
                .send(TextEndpoint::Messages, &body, true, prompt.text.timeout)
                .await?))?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            Ok(crate::native_tool_stream::anthropic(
                response.into_stream().map_err(transport_error)?,
            ))
        })
    }
}

impl AnthropicTextProvider {
    fn tool_payload(
        prompt: &AgentPrompt<'_>,
        tools: &[ToolDefinition],
        history: &[ToolStep],
    ) -> AiResult<Value> {
        let mut body = Self::prompt_body(prompt)?;
        let messages = body["messages"]
            .as_array_mut()
            .ok_or(AiError::InvalidResponse)?;
        for step in history {
            let content = step
                .provider_content
                .as_ref()
                .ok_or(AiError::InvalidResponse)?;
            messages.push(json!({"role":"assistant","content":content}));
            let results: Vec<_> = step
                .results
                .iter()
                .map(|result| {
                    json!({
                        "type":"tool_result","tool_use_id":result.id,
                        "content":result.result,"is_error":result.failed,
                    })
                })
                .collect();
            messages.push(json!({"role":"user","content":results}));
        }
        body["tools"] = tools
            .iter()
            .map(|tool| {
                json!({
                    "name":tool.name,"description":tool.description,"input_schema":tool.schema,
                })
            })
            .collect::<Vec<_>>()
            .into();

        if let Some(choice) = prompt.tool_choice.filter(|_| !tools.is_empty()) {
            if !body["thinking"].is_null()
                && matches!(choice, ToolChoice::Required | ToolChoice::Tool(_))
            {
                return Err(AiError::InvalidToolChoice);
            }
            body["tool_choice"] = match choice {
                ToolChoice::Auto => json!({"type":"auto"}),
                ToolChoice::None => json!({"type":"none"}),
                ToolChoice::Required => json!({"type":"any"}),
                ToolChoice::Tool(name) => json!({"type":"tool","name":name}),
            };
        }

        Ok(body)
    }
}
pub(crate) fn parse_step(mut response: Value) -> AiResult<StepResponse> {
    if response["stop_reason"] == "refusal" {
        return Err(AiError::Refused);
    }
    let content = response["content"].take();
    let blocks = content.as_array().ok_or(AiError::InvalidResponse)?;
    let mut tool_calls = Vec::new();
    let mut text = None::<String>;
    for block in blocks {
        match block["type"].as_str() {
            Some("tool_use") => tool_calls.push(ToolCall {
                id: block["id"].as_str().ok_or(AiError::InvalidResponse)?.into(),
                name: block["name"]
                    .as_str()
                    .ok_or(AiError::InvalidResponse)?
                    .into(),
                arguments: block["input"]
                    .as_object()
                    .ok_or(AiError::InvalidToolArguments)?
                    .clone(),
            }),
            Some("text") => text
                .get_or_insert_default()
                .push_str(block["text"].as_str().ok_or(AiError::InvalidResponse)?),
            _ => {}
        }
    }
    if (response["stop_reason"] == "tool_use") == tool_calls.is_empty() {
        return Err(AiError::InvalidResponse);
    }
    Ok(StepResponse {
        model: response["model"].as_str().map(str::to_owned),
        text,
        usage: Usage::from_anthropic(&response["usage"]).include_cached_input(),
        tool_calls,
        provider_content: Some(content),
    })
}
