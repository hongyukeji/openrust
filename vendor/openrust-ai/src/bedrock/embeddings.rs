use openrust::http::HttpFuture;
use serde::Deserialize;
use serde_json::json;

use crate::{
    AiError, AiResult, EmbeddingProvider, EmbeddingsPrompt, EmbeddingsResponse, StepTextGateway,
    Usage, embeddings::valid_vectors,
};

use super::BedrockProvider;

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
                let (payload, tokens) = self
                    .invoke_model(prompt.model, prompt.timeout, body)
                    .await?;
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
                    let (payload, _) = self
                        .invoke_model(prompt.model, prompt.timeout, body)
                        .await?;
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
