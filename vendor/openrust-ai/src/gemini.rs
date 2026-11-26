use std::{collections::HashSet, sync::Arc, time::Duration};

use openrust::{
    config::Environment,
    http::{
        HttpFuture,
        client::{Factory, PendingRequest, Response},
    },
    support::Str,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AgentPrompt, AgentResponse, AiError, AiResult, Attachment, EmbeddingProvider, MessageRole,
    ModelCatalog, StepEventStream, StepResponse, StepTextGateway, StreamableAgentResponse,
    TextEndpoint, TextModelTier, TextPrompt, TextProvider, ToolCall, ToolChoice, ToolDefinition,
    ToolResult, ToolStep, UrlCitation, Usage,
    classification::{ClassificationProvider, DecisionsProtocol, HttpDecisionsProvider},
    connection::{Authorization, Connection},
    files::{FileProvider, GeminiFileProvider},
    models::ListedModel,
    provider_options::apply_provider_options,
    stores::StoreProvider,
    text::{send_with_retry_after, transport_error, validate},
};

mod audio;
mod embeddings;
mod image;
mod interactions;

pub(crate) struct GeminiTextProvider(Arc<Connection>);

impl GeminiTextProvider {
    pub(crate) fn new(
        config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> AiResult<Self> {
        Ok(Self(Arc::new(Connection::new(
            config,
            environment,
            http,
            Authorization::Gemini,
        )?)))
    }

    fn model_request(
        &self,
        endpoint: TextEndpoint,
        model: &str,
        timeout: Option<Duration>,
    ) -> AiResult<(PendingRequest, String)> {
        if !self.supports_endpoint(endpoint) {
            return Err(AiError::EndpointNotSupported);
        }
        let model = validated_model_id(model)?;
        let path = format!("models/{model}:{}", endpoint.path());
        let mut request = self.0.request(timeout)?;
        if endpoint == TextEndpoint::GeminiStreamGenerateContent {
            request = request
                .with_query_parameters([("alt".into(), "sse".into())])
                .stream();
        }
        Ok((request, path))
    }

    async fn send_sdk_text(
        &self,
        endpoint: TextEndpoint,
        model: &str,
        body: &Value,
        timeout: Option<Duration>,
    ) -> AiResult<Response> {
        send_with_retry_after(endpoint, || async {
            let (request, path) = self.model_request(endpoint, model, timeout)?;
            request
                .distinguish_connection_setup_failure()
                .post(&path, body)
                .await
                .map_err(transport_error)
        })
        .await
    }

    fn message_parts(text: &str, attachments: &[Attachment]) -> Vec<Value> {
        let mut parts = Vec::new();
        if attachments.is_empty() || !text.trim().is_empty() {
            parts.push(json!({"text":text}));
        }
        parts.extend(attachments.iter().map(|attachment| {
            let (mime, content) = match attachment {
                Attachment::Image(image) => (image.mime(), image.content()),
                Attachment::Document(document) => (document.mime(), document.content()),
            };
            json!({"inlineData":{"mimeType":mime,"data":Str::to_base64(content)}})
        }));
        parts
    }

    fn function_result(result: &ToolResult, id: Option<Value>) -> Value {
        let payload = if let Some(error) = result.error() {
            json!({"error":error})
        } else {
            json!({"output":result.text()})
        };
        let mut response = json!({"name":result.name,"response":payload});
        if let Some(id) = id {
            response["id"] = id;
        }
        json!({"functionResponse":response})
    }

    fn prompt_body(&self, prompt: &AgentPrompt<'_>) -> AiResult<Value> {
        let scope = prompt
            .messages
            .iter()
            .any(|message| message.provider_replay.is_some())
            .then(|| self.approval_replay_scope())
            .transpose()?
            .flatten();
        let mut native_call_ids = None::<Vec<Option<Value>>>;
        let mut contents = Vec::with_capacity(prompt.messages.len() + 1);
        for message in prompt.messages {
            let native_ids = native_call_ids.take();
            if let Some(content) = scope.as_deref().and_then(|scope| {
                message
                    .provider_replay
                    .as_ref()?
                    .gemini_content(scope, prompt.text.model)
            }) {
                let parts = content["parts"]
                    .as_array()
                    .ok_or(AiError::InvalidResponse)?;
                native_call_ids = Some(
                    parts
                        .iter()
                        .filter_map(|part| part.get("functionCall"))
                        .map(|call| call.get("id").cloned())
                        .collect(),
                );
                contents.push(content.clone());
                continue;
            }
            let (role, parts) = match message.role {
                MessageRole::User => (
                    "user",
                    Self::message_parts(&message.content, &message.attachments),
                ),
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if message.tool_calls.is_empty() || !message.content.is_empty() {
                        parts.push(json!({"text":message.content}));
                    }
                    parts.extend(message.tool_calls.iter().map(|call| {
                        json!({
                            "functionCall":{"id":call.wire_id(),"name":call.name,"args":call.arguments}
                        })
                    }));
                    ("model", parts)
                }
                MessageRole::ToolResult => (
                    "user",
                    message
                        .tool_results
                        .iter()
                        .enumerate()
                        .map(|(index, result)| {
                            let id = native_ids
                                .as_ref()
                                .and_then(|ids| ids.get(index))
                                .cloned()
                                .unwrap_or_else(|| Some(Value::from(result.wire_id())));
                            Self::function_result(result, id)
                        })
                        .collect(),
                ),
            };
            contents.push(json!({"role":role,"parts":parts}));
        }
        let parts = Self::message_parts(prompt.text.prompt, prompt.attachments);
        contents.push(json!({"role":"user","parts":parts}));
        let mut body = json!({"contents":contents});
        if !prompt.text.instructions.is_empty() {
            body["systemInstruction"] = json!({"parts":[{"text":prompt.text.instructions}]});
        }
        if let Some(output) = &prompt.structured_output {
            body["generationConfig"] = json!({"responseMimeType":"application/json","responseJsonSchema":output.object_schema()});
        }
        if let Some(max_tokens) = prompt.options.max_tokens {
            body["generationConfig"]["maxOutputTokens"] = max_tokens.into();
        }
        if let Some(temperature) = prompt.options.temperature {
            body["generationConfig"]["temperature"] = temperature.into();
        }
        if let Some(top_p) = prompt.options.top_p {
            body["generationConfig"]["topP"] = top_p.into();
        }
        apply_provider_options(&mut body, prompt.provider_options, true)?;
        Ok(body)
    }

    async fn list_models(&self) -> AiResult<Vec<ListedModel>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            models: Vec<Model>,
            next_page_token: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Model {
            name: String,
            display_name: Option<String>,
        }

        let mut models = Vec::new();
        let mut cursor = None::<String>;
        let mut seen = HashSet::new();
        for _ in 0..100 {
            let mut request = self.0.request(None)?;
            if let Some(cursor) = &cursor {
                request = request.with_query_parameters([("pageToken".into(), cursor.clone())]);
            }
            let response = validate(request.get("models").await)?;
            let page: Page = response.json().map_err(|_| AiError::InvalidResponse)?;
            models.extend(page.models.into_iter().map(|model| {
                ListedModel {
                    id: model
                        .name
                        .strip_prefix("models/")
                        .unwrap_or(&model.name)
                        .into(),
                    name: model.display_name,
                    input_modalities: None,
                    output_modalities: None,
                }
            }));
            let Some(next) = page.next_page_token.filter(|token| !token.is_empty()) else {
                return Ok(models);
            };
            if !seen.insert(next.clone()) {
                return Err(AiError::InvalidResponse);
            }
            cursor = Some(next);
        }
        Err(AiError::InvalidResponse)
    }
}

impl TextProvider for GeminiTextProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        self.0.default_text_model()
    }

    fn text_model(&self, tier: TextModelTier) -> AiResult<&str> {
        self.0.text_model(tier)
    }

    fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        Some(Arc::new(embeddings::GeminiEmbeddingProvider(
            self.0.clone(),
        )))
    }

    fn audio_provider(&self) -> Option<Arc<dyn crate::AudioProvider>> {
        Some(Arc::new(audio::GeminiAudioProvider(self.0.clone())))
    }

    fn transcription_provider(&self) -> Option<Arc<dyn crate::TranscriptionProvider>> {
        Some(Arc::new(audio::GeminiAudioProvider(self.0.clone())))
    }

    fn image_provider(&self) -> Option<Arc<dyn crate::ImageProvider>> {
        Some(Arc::new(image::GeminiImageProvider(self.0.clone())))
    }

    fn file_provider(&self) -> Option<Arc<dyn FileProvider>> {
        Some(Arc::new(GeminiFileProvider(self.0.clone())))
    }

    fn store_provider(&self) -> Option<Arc<dyn StoreProvider>> {
        Some(Arc::new(GeminiFileProvider(self.0.clone())))
    }

    fn classification_provider(&self) -> Option<Arc<dyn ClassificationProvider>> {
        Some(Arc::new(HttpDecisionsProvider {
            connection: self.0.clone(),
            protocol: DecisionsProtocol::Gemini,
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
            let response = self
                .send_sdk_text(
                    TextEndpoint::GeminiGenerateContent,
                    prompt.text.model,
                    &self.prompt_body(&prompt)?,
                    prompt.text.timeout,
                )
                .await?;
            let value: Value = response.json().map_err(|_| AiError::InvalidResponse)?;
            let candidate = &value["candidates"][0];
            let reason = finish_reason(&value, candidate)?;
            let text: String = text_parts(candidate).collect();
            if text.is_empty() {
                return Err(AiError::InvalidResponse);
            }
            Ok(AgentResponse {
                text,
                reasoning: String::new(),
                usage: usage(&value),
                provider: self.name().into(),
                model: value["modelVersion"]
                    .as_str()
                    .unwrap_or(prompt.text.model)
                    .into(),
                citations: citations(candidate),
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                steps: Vec::new(),
            }
            .with_step(crate::FinishReason::from_wire(reason.as_deref())))
        })
    }

    fn stream<'a>(
        &'a self,
        prompt: AgentPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        Box::pin(async move {
            let response = self
                .send_sdk_text(
                    TextEndpoint::GeminiStreamGenerateContent,
                    prompt.text.model,
                    &self.prompt_body(&prompt)?,
                    prompt.text.timeout,
                )
                .await?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            Ok(StreamableAgentResponse::from_upstream(
                TextEndpoint::GeminiStreamGenerateContent,
                response.into_stream().map_err(transport_error)?,
                self.name().into(),
                prompt.text.model.to_owned(),
                false,
            ))
        })
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        Some(self)
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async {
            ModelCatalog::merge(
                self.list_models().await,
                self.0
                    .configured_text_models()
                    .map(model_id)
                    .map(str::to_owned),
            )
        })
    }

    fn supports_endpoint(&self, endpoint: TextEndpoint) -> bool {
        matches!(
            endpoint,
            TextEndpoint::GeminiGenerateContent
                | TextEndpoint::GeminiStreamGenerateContent
                | TextEndpoint::GeminiCountTokens
                | TextEndpoint::GeminiEmbedContent
                | TextEndpoint::GeminiBatchEmbedContents
        )
    }

    fn relay(&self, endpoint: TextEndpoint, mut body: Value) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async move {
            let model = body
                .as_object_mut()
                .and_then(|body| body.remove("model"))
                .and_then(|model| model.as_str().map(str::to_owned))
                .ok_or(AiError::MissingModel)?;
            if endpoint.is_gemini_embedding() {
                let resource = format!("models/{}", validated_model_id(&model)?);
                match endpoint {
                    TextEndpoint::GeminiEmbedContent => body["model"] = resource.into(),
                    TextEndpoint::GeminiBatchEmbedContents => {
                        let requests = body["requests"]
                            .as_array_mut()
                            .filter(|requests| !requests.is_empty())
                            .ok_or(AiError::InvalidPrompt)?;
                        for request in requests {
                            let request = request.as_object_mut().ok_or(AiError::InvalidPrompt)?;
                            request.insert("model".into(), resource.as_str().into());
                        }
                    }
                    _ => unreachable!(),
                }
            }
            let (request, path) = self.model_request(endpoint, &model, None)?;
            request.post(&path, &body).await.map_err(transport_error)
        })
    }
}

fn validated_model_id(model: &str) -> AiResult<&str> {
    let model = model_id(model);
    if model.is_empty()
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || !model
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
    {
        return Err(AiError::InvalidConfiguration("providers.<name>.models"));
    }
    Ok(model)
}

fn model_id(name: &str) -> &str {
    name.strip_prefix("models/").unwrap_or(name)
}

pub(crate) fn finish_reason(value: &Value, candidate: &Value) -> AiResult<Option<String>> {
    if value["promptFeedback"]["blockReason"]
        .as_str()
        .is_some_and(|reason| reason != "BLOCK_REASON_UNSPECIFIED")
    {
        return Err(AiError::Refused);
    }
    Ok(match candidate["finishReason"].as_str() {
        None | Some("FINISH_REASON_UNSPECIFIED") => None,
        Some("STOP") => Some("stop".into()),
        Some("MAX_TOKENS") => Some("length".into()),
        Some("SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" | "RECITATION") => {
            return Err(AiError::Refused);
        }
        Some(_) => return Err(AiError::InvalidResponse),
    })
}

pub(crate) fn text_parts(candidate: &Value) -> impl Iterator<Item = &str> {
    candidate["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| part["thought"] != true)
        .filter_map(|part| part["text"].as_str())
}

pub(crate) fn usage(value: &Value) -> Usage {
    Usage::from_gemini(&value["usageMetadata"]).include_reasoning_output()
}

impl StepTextGateway for GeminiTextProvider {
    fn approval_replay_scope(&self) -> AiResult<Option<String>> {
        self.0.request_namespace("gemini-tool-replay").map(Some)
    }

    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        Box::pin(async move {
            let body = self.tool_payload(prompt, tools, history)?;

            let value: Value = self
                .send_sdk_text(
                    TextEndpoint::GeminiGenerateContent,
                    prompt.text.model,
                    &body,
                    prompt.text.timeout,
                )
                .await?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            parse_step(value)
        })
    }

    fn stream_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepEventStream>> {
        Box::pin(async move {
            let body = self.tool_payload(prompt, tools, history)?;
            let response = self
                .send_sdk_text(
                    TextEndpoint::GeminiStreamGenerateContent,
                    prompt.text.model,
                    &body,
                    prompt.text.timeout,
                )
                .await?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            Ok(crate::native_tool_stream::gemini(
                response.into_stream().map_err(transport_error)?,
            ))
        })
    }
}

impl GeminiTextProvider {
    fn tool_payload(
        &self,
        prompt: &AgentPrompt<'_>,
        tools: &[ToolDefinition],
        history: &[ToolStep],
    ) -> AiResult<Value> {
        let mut body = self.prompt_body(prompt)?;
        let contents = body["contents"]
            .as_array_mut()
            .ok_or(AiError::InvalidResponse)?;
        for step in history {
            let content = step
                .provider_content
                .as_ref()
                .ok_or(AiError::InvalidResponse)?;
            let parts = content["parts"]
                .as_array()
                .ok_or(AiError::InvalidResponse)?;
            let calls = parts.iter().filter_map(|part| part.get("functionCall"));
            let results = calls
                .zip(&step.results)
                .map(|(call, result)| Self::function_result(result, call.get("id").cloned()))
                .collect::<Vec<_>>();
            contents.push(content.clone());
            contents.push(json!({"role":"user","parts":results}));
        }
        let declarations: Vec<_> = tools.iter().map(|tool| json!({
                "name":tool.name, "description":tool.description,"parametersJsonSchema":tool.schema,
            })).collect();
        if !declarations.is_empty() {
            body["tools"] = json!([{"functionDeclarations":declarations}]);
        }

        if let Some(choice) = prompt.tool_choice.filter(|_| !tools.is_empty()) {
            body["toolConfig"]["functionCallingConfig"] = match choice {
                ToolChoice::Auto => json!({"mode":"AUTO"}),
                ToolChoice::None => json!({"mode":"NONE"}),
                ToolChoice::Required => json!({"mode":"ANY"}),
                ToolChoice::Tool(name) => json!({"mode":"ANY","allowedFunctionNames":[name]}),
            };
        }

        Ok(body)
    }
}
pub(crate) fn parse_step(mut value: Value) -> AiResult<StepResponse> {
    let candidate = &value["candidates"][0];
    let reason = finish_reason(&value, candidate)?;
    let citations = citations(candidate);
    let text = text_parts(candidate).fold(None::<String>, |mut text, part| {
        text.get_or_insert_default().push_str(part);
        text
    });
    let mut tool_calls = Vec::new();
    let content = value
        .get_mut("candidates")
        .and_then(Value::as_array_mut)
        .and_then(|candidates| candidates.first_mut())
        .and_then(|candidate| candidate.get_mut("content"))
        .map(Value::take)
        .ok_or(AiError::InvalidResponse)?;
    for function in content["parts"]
        .as_array()
        .ok_or(AiError::InvalidResponse)?
        .iter()
        .filter_map(|part| part.get("functionCall"))
    {
        if reason.as_deref() != Some("stop") {
            return Err(AiError::InvalidResponse);
        }
        let id = match function.get("id") {
            Some(id) => id.as_str().ok_or(AiError::InvalidResponse)?.into(),
            None => Str::ulid().to_string(),
        };
        if function["willContinue"] == true || !function["partialArgs"].is_null() {
            return Err(AiError::InvalidToolArguments);
        }
        let arguments = match function.get("args") {
            Some(args) => args
                .as_object()
                .ok_or(AiError::InvalidToolArguments)?
                .clone(),
            None => Default::default(),
        };
        tool_calls.push(ToolCall {
            result_id: None,
            id,
            name: function["name"]
                .as_str()
                .ok_or(AiError::InvalidResponse)?
                .into(),
            arguments,
        });
    }
    Ok(StepResponse {
        model: value["modelVersion"].as_str().map(str::to_owned),
        text,
        finish_reason: if tool_calls.is_empty() {
            crate::FinishReason::from_wire(reason.as_deref())
        } else {
            crate::FinishReason::ToolCalls
        },
        reasoning: String::new(),
        usage: usage(&value),
        citations,
        tool_calls,
        provider_content: Some(content),
    })
}

pub(crate) fn citations(candidate: &Value) -> Vec<UrlCitation> {
    let sources = candidate["groundingMetadata"]["groundingChunks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|chunk| (&chunk["web"]["uri"], &chunk["web"]["title"]));
    UrlCitation::unique_sources(sources)
}
