use std::sync::{Arc, OnceLock};

use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        interceptors::{Intercept, context::BeforeDeserializationInterceptorContextRef},
        runtime_components::RuntimeComponents,
    },
};
use openrust::http::HttpFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, EmbeddingProvider, EmbeddingsPrompt, EmbeddingsResponse, StepTextGateway,
    Usage, embeddings::valid_vectors,
};

use super::{BedrockProvider, Blob, TimeoutConfig, converse_error};

impl EmbeddingProvider for BedrockProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_embeddings_model(&self) -> AiResult<&str> {
        self.connection.default_embeddings_model()
    }

    fn default_embeddings_dimensions(&self) -> Option<usize> {
        self.connection
            .default_embeddings_dimensions()
            .or(Some(1024))
    }

    fn normalize_embeddings_dimensions(
        &self,
        model: &str,
        dimensions: Option<usize>,
    ) -> Option<usize> {
        if is_cohere(model) { None } else { dimensions }
    }

    fn embeddings_cache_namespace(&self) -> AiResult<Option<String>> {
        Ok(self
            .approval_replay_scope()?
            .map(|scope| format!("bedrock-embeddings:{scope}")))
    }

    fn embeddings<'a>(
        &'a self,
        prompt: EmbeddingsPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<EmbeddingsResponse>> {
        Box::pin(async move {
            if prompt.inputs.is_empty() || prompt.inputs.iter().any(|input| input.trim().is_empty())
            {
                return Err(AiError::InvalidPrompt);
            }
            let cohere = is_cohere(prompt.model);
            let dimensions = prompt.dimensions.unwrap_or(1024);
            let (embeddings, tokens) = if cohere {
                let body = json!({"input_type":"search_document","texts":prompt.inputs});
                let (payload, tokens) = self.invoke_embeddings(prompt, body).await?;
                let vectors = &payload["embeddings"];
                let vectors = if vectors.is_object() {
                    &vectors["float"]
                } else {
                    vectors
                };
                (
                    Vec::<Vec<f32>>::deserialize(vectors).map_err(|_| AiError::InvalidResponse)?,
                    tokens,
                )
            } else {
                let mut embeddings = Vec::with_capacity(prompt.inputs.len());
                let mut total = Some(0_u64);
                for input in prompt.inputs {
                    let body = json!({"inputText":input,"dimensions":dimensions});
                    let (payload, _) = self.invoke_embeddings(prompt, body).await?;
                    embeddings.push(
                        Vec::<f32>::deserialize(&payload["embedding"])
                            .map_err(|_| AiError::InvalidResponse)?,
                    );
                    total = match (total, payload["inputTextTokenCount"].as_u64()) {
                        (Some(total), Some(tokens)) => {
                            Some(total.checked_add(tokens).ok_or(AiError::InvalidResponse)?)
                        }
                        _ => None,
                    };
                }
                (embeddings, total)
            };
            if !valid_vectors(
                &embeddings,
                prompt.inputs.len(),
                (!cohere).then_some(dimensions),
            ) {
                return Err(AiError::InvalidResponse);
            }
            Ok(EmbeddingsResponse {
                embeddings,
                usage: Usage::new(tokens, None),
                provider: self.connection.name().into(),
                model: prompt.model.into(),
            })
        })
    }
}

fn is_cohere(model: &str) -> bool {
    model.contains("cohere.embed-")
}

impl BedrockProvider {
    async fn invoke_embeddings(
        &self,
        prompt: EmbeddingsPrompt<'_>,
        body: Value,
    ) -> AiResult<(Value, Option<u64>)> {
        let input_tokens = Arc::new(OnceLock::new());
        let timeout = TimeoutConfig::builder()
            .operation_timeout(prompt.timeout)
            .build();
        let output = self
            .client()
            .await?
            .invoke_model()
            .model_id(prompt.model)
            .content_type("application/json")
            .accept("application/json")
            .body(Blob::new(
                serde_json::to_vec(&body).map_err(|_| AiError::InvalidPrompt)?,
            ))
            .customize()
            .config_override(aws_sdk_bedrockruntime::config::Builder::new().timeout_config(timeout))
            .interceptor(InputTokenHeader(input_tokens.clone()))
            .send()
            .await
            .map_err(converse_error)?;
        let body =
            serde_json::from_slice(output.body().as_ref()).map_err(|_| AiError::InvalidResponse)?;
        Ok((body, input_tokens.get().copied()))
    }
}

#[derive(Debug)]
struct InputTokenHeader(Arc<OnceLock<u64>>);

impl Intercept for InputTokenHeader {
    fn name(&self) -> &'static str {
        "BedrockInputTokenHeader"
    }

    fn read_before_deserialization(
        &self,
        context: &BeforeDeserializationInterceptorContextRef<'_>,
        _: &RuntimeComponents,
        _: &mut aws_sdk_bedrockruntime::config::ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(tokens) = context
            .response()
            .headers()
            .get("x-amzn-bedrock-input-token-count")
            .and_then(|value| value.parse().ok())
        {
            let _ = self.0.set(tokens);
        }
        Ok(())
    }
}
