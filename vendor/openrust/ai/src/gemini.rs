use std::{collections::HashSet, sync::Arc, time::Duration};

use openrust::{
    config::Environment,
    http::{
        HttpFuture,
        client::{Factory, Response},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AgentPrompt, AgentResponse, AiError, AiResult, MessageRole, ModelCatalog,
    StreamableAgentResponse, TextEndpoint, TextPrompt, TextProvider, Usage,
    connection::{Authorization, Connection},
    models::ListedModel,
    text::{transport_error, validate},
};

pub(crate) struct GeminiTextProvider(Connection);

impl GeminiTextProvider {
    pub(crate) fn new(
        config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> AiResult<Self> {
        Ok(Self(Connection::new(
            config,
            environment,
            http,
            Authorization::Gemini,
        )?))
    }

    async fn relay_model(
        &self,
        endpoint: TextEndpoint,
        model: &str,
        body: &Value,
        timeout: Option<Duration>,
    ) -> AiResult<Response> {
        if !self.supports_endpoint(endpoint) {
            return Err(AiError::EndpointNotSupported);
        }
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
        let path = format!("models/{model}:{}", endpoint.path());
        let mut request = self.0.request(timeout)?;
        if endpoint == TextEndpoint::GeminiStreamGenerateContent {
            request = request
                .with_query_parameters([("alt".into(), "sse".into())])
                .stream();
        }
        request.post(&path, body).await.map_err(transport_error)
    }

    fn prompt_body(prompt: &AgentPrompt<'_>) -> Value {
        let mut contents: Vec<_> = prompt
            .messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "model",
                };
                json!({"role":role,"parts":[{"text":message.content}]})
            })
            .collect();
        contents.push(json!({"role":"user","parts":[{"text":prompt.text.prompt}]}));
        let mut body = json!({"contents":contents});
        if !prompt.text.instructions.is_empty() {
            body["systemInstruction"] = json!({"parts":[{"text":prompt.text.instructions}]});
        }
        if let Some(output) = &prompt.structured_output {
            body["generationConfig"] =
                json!({"responseMimeType":"application/json","responseJsonSchema":output.schema});
        }
        body
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
                .relay_model(
                    TextEndpoint::GeminiGenerateContent,
                    prompt.text.model,
                    &Self::prompt_body(&prompt),
                    prompt.text.timeout,
                )
                .await?;
            let value: Value = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let candidate = &value["candidates"][0];
            finish_reason(&value, candidate)?;
            let text: String = text_parts(candidate).collect();
            if text.is_empty() {
                return Err(AiError::InvalidResponse);
            }
            Ok(AgentResponse {
                text,
                usage: usage(&value),
                provider: self.name().into(),
                model: value["modelVersion"]
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
            let response = validate(Ok(self
                .relay_model(
                    TextEndpoint::GeminiStreamGenerateContent,
                    prompt.text.model,
                    &Self::prompt_body(&prompt),
                    prompt.text.timeout,
                )
                .await?))?;
            if !crate::streaming::is_event_stream(&response) {
                return Err(AiError::InvalidResponse);
            }
            Ok(StreamableAgentResponse::from_upstream(
                TextEndpoint::GeminiStreamGenerateContent,
                response.into_stream().map_err(transport_error)?,
                self.name().into(),
                prompt.text.model.to_owned(),
            ))
        })
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
        )
    }

    fn relay(&self, endpoint: TextEndpoint, mut body: Value) -> HttpFuture<'_, AiResult<Response>> {
        Box::pin(async move {
            let model = body
                .as_object_mut()
                .and_then(|body| body.remove("model"))
                .and_then(|model| model.as_str().map(str::to_owned))
                .ok_or(AiError::MissingModel)?;
            self.relay_model(endpoint, &model, &body, None).await
        })
    }
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
    let usage = &value["usageMetadata"];
    Usage {
        prompt_tokens: usage["promptTokenCount"].as_u64(),
        completion_tokens: usage["candidatesTokenCount"].as_u64(),
    }
}
