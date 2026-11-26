use std::{collections::HashSet, sync::Arc};

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
    AgentResponse, AiError, AiResult, ModelCatalog, StreamableAgentResponse, TextEndpoint,
    TextPrompt, TextProvider, Usage,
    connection::{Authorization, Connection},
    models::ListedModel,
    text::{transport_error, validate},
};

pub(crate) struct AnthropicTextProvider(Connection);

impl AnthropicTextProvider {
    pub(crate) fn new(
        config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> AiResult<Self> {
        Ok(Self(Connection::new(
            config,
            environment,
            http,
            Authorization::Anthropic,
        )?))
    }

    async fn send(&self, prompt: &TextPrompt<'_>, stream: bool) -> AiResult<Response> {
        let mut body = json!({
            "model": prompt.model,
            "max_tokens": 64_000,
            "messages": [{"role": "user", "content": prompt.prompt}],
        });
        if !prompt.instructions.is_empty() {
            body["system"] = prompt.instructions.into();
        }
        if stream {
            body["stream"] = true.into();
        }
        validate(Ok(self
            .0
            .send(TextEndpoint::Messages, &body, stream, prompt.timeout)
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

impl TextProvider for AnthropicTextProvider {
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
                usage: Usage {
                    prompt_tokens: response["usage"]["input_tokens"].as_u64(),
                    completion_tokens: response["usage"]["output_tokens"].as_u64(),
                },
                provider: self.name().into(),
                model: response["model"].as_str().unwrap_or(prompt.model).into(),
            })
        })
    }

    fn stream_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
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
                prompt.model.to_owned(),
            ))
        })
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
