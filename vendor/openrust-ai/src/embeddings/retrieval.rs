use std::sync::Arc;

use openrust::http::HttpFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, EmbeddingProvider, EmbeddingsPrompt, EmbeddingsResponse, Usage,
    connection::Connection, text::validate,
};

use super::{IndexedVector, indexed_vectors, valid_vectors};

#[derive(Clone, Copy)]
pub(crate) enum RetrievalEmbeddingProtocol {
    Cohere,
    Jina,
    Voyage,
}

pub(crate) struct RetrievalEmbeddingProvider {
    pub(crate) connection: Arc<Connection>,
    pub(crate) protocol: RetrievalEmbeddingProtocol,
}

impl EmbeddingProvider for RetrievalEmbeddingProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_embeddings_model(&self) -> AiResult<&str> {
        self.connection.default_embeddings_model()
    }

    fn default_embeddings_dimensions(&self) -> Option<usize> {
        self.connection.default_embeddings_dimensions()
    }

    fn embeddings_cache_namespace(&self) -> AiResult<Option<String>> {
        self.connection
            .request_namespace(match self.protocol {
                RetrievalEmbeddingProtocol::Cohere => "cohere-embeddings",
                RetrievalEmbeddingProtocol::Jina => "jina-embeddings",
                RetrievalEmbeddingProtocol::Voyage => "voyage-embeddings",
            })
            .map(Some)
    }

    fn embeddings<'a>(
        &'a self,
        prompt: EmbeddingsPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<EmbeddingsResponse>> {
        Box::pin(async move {
            if prompt.inputs.is_empty() || prompt.inputs.iter().any(|text| text.trim().is_empty()) {
                return Err(AiError::InvalidPrompt);
            }
            let (endpoint, body) = self.protocol.request(prompt)?;
            let payload: Value = validate(
                self.connection
                    .request(Some(prompt.timeout))?
                    .post(endpoint, &body)
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            let (embeddings, tokens) = match self.protocol {
                RetrievalEmbeddingProtocol::Cohere => (
                    parse_cohere_vectors(&payload)?,
                    payload["meta"]["billed_units"]["input_tokens"].as_u64(),
                ),
                _ => {
                    #[derive(Deserialize)]
                    struct IndexedResponse {
                        data: Vec<IndexedVector>,
                    }
                    let vectors: IndexedResponse = IndexedResponse::deserialize(&payload)
                        .map_err(|_| AiError::InvalidResponse)?;
                    (
                        indexed_vectors(vectors.data, prompt.inputs.len())?,
                        payload["usage"]["total_tokens"]
                            .as_u64()
                            .or_else(|| payload["total_tokens"].as_u64()),
                    )
                }
            };
            let dimensions = if matches!(self.protocol, RetrievalEmbeddingProtocol::Voyage)
                && prompt.model == "voyage-multimodal-3"
            {
                Some(1024)
            } else {
                prompt.dimensions
            };
            if !valid_vectors(&embeddings, prompt.inputs.len(), dimensions) {
                return Err(AiError::InvalidResponse);
            }
            Ok(EmbeddingsResponse {
                embeddings,
                usage: Usage::new(tokens, None),
                provider: self.name().into(),
                model: payload["model"].as_str().unwrap_or(prompt.model).into(),
            })
        })
    }
}

impl RetrievalEmbeddingProtocol {
    fn request(self, prompt: EmbeddingsPrompt<'_>) -> AiResult<(&'static str, Value)> {
        let legacy_voyage = prompt.model == "voyage-multimodal-3";
        let (endpoint, mut body) = match self {
            Self::Cohere => (
                "embed",
                json!({
                    "model":prompt.model,"texts":prompt.inputs,"input_type":"search_document","embedding_types":["float"],
                }),
            ),
            Self::Jina => {
                let input = prompt
                    .inputs
                    .iter()
                    .map(|text| json!({"text":text}))
                    .collect::<Vec<_>>();
                (
                    "embeddings",
                    json!({"model":prompt.model,"input":input,"task":"retrieval.passage"}),
                )
            }
            Self::Voyage
                if matches!(
                    prompt.model,
                    "voyage-multimodal-3" | "voyage-multimodal-3.5"
                ) =>
            {
                if legacy_voyage
                    && prompt
                        .dimensions
                        .is_some_and(|dimensions| dimensions != 1024)
                {
                    return Err(AiError::InvalidConfiguration("embeddings.dimensions"));
                }
                let inputs = prompt
                    .inputs
                    .iter()
                    .map(|text| json!({"content":[{"type":"text","text":text}]}))
                    .collect::<Vec<_>>();
                (
                    "multimodalembeddings",
                    json!({"model":prompt.model,"inputs":inputs}),
                )
            }
            Self::Voyage => (
                "embeddings",
                json!({"model":prompt.model,"input":prompt.inputs}),
            ),
        };
        if let Some(dimensions) = prompt.dimensions {
            match self {
                Self::Jina => body["dimensions"] = dimensions.into(),
                Self::Voyage if !legacy_voyage => body["output_dimension"] = dimensions.into(),
                _ => {}
            }
        }
        Ok((endpoint, body))
    }
}

fn parse_cohere_vectors(data: &Value) -> AiResult<Vec<Vec<f32>>> {
    let vectors = match &data["embeddings"] {
        value @ Value::Array(_) => value,
        value => &value["float"],
    };
    Vec::<Vec<f32>>::deserialize(vectors).map_err(|_| AiError::InvalidResponse)
}
