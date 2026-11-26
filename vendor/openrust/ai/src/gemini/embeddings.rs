use std::sync::Arc;

use openrust::http::HttpFuture;
use serde::Deserialize;
use serde_json::json;

use crate::{
    AiError, AiResult, EmbeddingProvider, EmbeddingsPrompt, EmbeddingsResponse, Usage,
    connection::Connection,
    embeddings::valid_vectors,
    text::{transport_error, validate},
};

use super::validated_model_id;

pub(super) struct GeminiEmbeddingProvider(pub(super) Arc<Connection>);

impl EmbeddingProvider for GeminiEmbeddingProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_embeddings_model(&self) -> AiResult<&str> {
        self.0.default_embeddings_model()
    }

    fn default_embeddings_dimensions(&self) -> Option<usize> {
        self.0.default_embeddings_dimensions()
    }

    fn normalize_embeddings_model<'a>(&self, model: &'a str) -> AiResult<&'a str> {
        validated_model_id(model)
    }

    fn embeddings_cache_namespace(&self) -> AiResult<Option<String>> {
        self.0
            .embeddings_cache_namespace("gemini-batch-embed-contents")
            .map(Some)
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
            let dimensions = prompt
                .dimensions
                .map(i32::try_from)
                .transpose()
                .map_err(|_| AiError::InvalidConfiguration("embeddings.dimensions"))?;
            if dimensions == Some(0) || prompt.timeout.is_zero() {
                return Err(AiError::InvalidConfiguration("embeddings.dimensions"));
            }
            let model = self.normalize_embeddings_model(prompt.model)?;
            let resource = format!("models/{model}");
            let requests: Vec<_> = prompt
                .inputs
                .iter()
                .map(|input| {
                    let mut entry = json!({"model":resource, "content":{"parts":[{"text":input}]}});
                    if let Some(dimensions) = dimensions {
                        entry["outputDimensionality"] = dimensions.into();
                    }
                    entry
                })
                .collect();
            let response = self
                .0
                .request(Some(prompt.timeout))?
                .post(
                    &format!("{resource}:batchEmbedContents"),
                    &json!({"requests":requests}),
                )
                .await
                .map_err(transport_error)?;
            let payload: Payload = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let embeddings: Vec<_> = payload
                .embeddings
                .into_iter()
                .map(|embedding| embedding.values)
                .collect();
            if !valid_vectors(&embeddings, prompt.inputs.len(), prompt.dimensions) {
                return Err(AiError::InvalidResponse);
            }
            Ok(EmbeddingsResponse {
                embeddings,
                usage: Usage {
                    prompt_tokens: payload
                        .usage_metadata
                        .and_then(|usage| usage.prompt_token_count),
                    completion_tokens: None,
                },
                provider: self.name().into(),
                model: model.into(),
            })
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    embeddings: Vec<Vector>,
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize)]
struct Vector {
    values: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageMetadata {
    prompt_token_count: Option<u64>,
}
