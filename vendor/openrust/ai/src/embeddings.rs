use std::{sync::Arc, time::Duration};

use async_openai::types::embeddings::{CreateEmbeddingRequest, EmbeddingInput, EncodingFormat};
use openrust::http::{HttpFuture, client::Response};
use serde::{Deserialize, Serialize};

use crate::{
    Ai, AiError, AiResult, TextEndpoint, Usage,
    connection::Connection,
    embedding_cache::{CacheOverride, EmbeddingCache},
    text::validate,
};

pub struct Embeddings;

impl Embeddings {
    pub fn r#for(
        inputs: impl IntoIterator<Item = impl Into<String>>,
    ) -> AiResult<PendingEmbeddingsGeneration> {
        let inputs = inputs.into_iter().map(Into::into).collect::<Vec<_>>();
        if inputs.is_empty() || inputs.iter().any(|input| input.trim().is_empty()) {
            return Err(AiError::InvalidPrompt);
        }
        Ok(PendingEmbeddingsGeneration {
            inputs,
            dimensions: None,
            timeout: Duration::from_secs(30),
            cache: None,
        })
    }
}

pub struct PendingEmbeddingsGeneration {
    inputs: Vec<String>,
    dimensions: Option<usize>,
    timeout: Duration,
    cache: Option<CacheOverride>,
}

impl PendingEmbeddingsGeneration {
    pub fn cache(mut self, seconds: Option<i64>, individually: Option<bool>) -> Self {
        let individually = if seconds.is_some_and(|seconds| seconds <= 0) {
            None
        } else {
            individually.or_else(|| self.cache.as_ref().and_then(|cache| cache.individually))
        };
        self.cache = Some(CacheOverride {
            seconds,
            individually,
        });
        self
    }

    pub fn dimensions(mut self, dimensions: usize) -> Self {
        self.dimensions = Some(dimensions);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn generate(self) -> AiResult<EmbeddingsResponse> {
        self.generate_with(None, None).await
    }

    pub async fn generate_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<EmbeddingsResponse> {
        if self.dimensions == Some(0) || self.timeout.is_zero() {
            return Err(AiError::InvalidConfiguration(
                "embeddings 维度与超时必须大于零",
            ));
        }
        let manager = Ai::manager()?;
        let provider = manager.embedding_provider(provider)?;
        let model = match model {
            Some(model) if !model.trim().is_empty() => model,
            Some(_) => return Err(AiError::MissingModel),
            None => provider.default_embeddings_model()?,
        };
        let prompt = EmbeddingsPrompt {
            inputs: &self.inputs,
            model,
            dimensions: self
                .dimensions
                .or_else(|| provider.default_embeddings_dimensions()),
            timeout: self.timeout,
        };
        if let Some(cache) = EmbeddingCache::resolve(
            manager.embedding_cache_config(),
            self.cache,
            provider.as_ref(),
        )? {
            return cache.generate(provider.as_ref(), prompt).await;
        }
        provider.embeddings(prompt).await
    }
}

#[derive(Clone, Copy)]
pub struct EmbeddingsPrompt<'a> {
    pub inputs: &'a [String],
    pub model: &'a str,
    pub dimensions: Option<usize>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EmbeddingsResponse {
    pub embeddings: Vec<Vec<f32>>,
    pub usage: Usage,
    pub provider: String,
    pub model: String,
}

impl EmbeddingsResponse {
    pub fn first(&self) -> Option<&[f32]> {
        self.embeddings.first().map(Vec::as_slice)
    }
}

pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_embeddings_model(&self) -> AiResult<&str>;
    fn default_embeddings_dimensions(&self) -> Option<usize>;

    /// 由驱动提供连接隔离标识。返回 None 时仅调用上游。
    fn embeddings_cache_namespace(&self) -> AiResult<Option<String>> {
        Ok(None)
    }

    fn embeddings<'a>(
        &'a self,
        prompt: EmbeddingsPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<EmbeddingsResponse>>;
}

pub(crate) struct OpenAiEmbeddingProvider(pub(crate) Arc<Connection>);

impl EmbeddingProvider for OpenAiEmbeddingProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_embeddings_model(&self) -> AiResult<&str> {
        self.0.default_embeddings_model()
    }

    fn default_embeddings_dimensions(&self) -> Option<usize> {
        self.0.default_embeddings_dimensions()
    }

    fn embeddings_cache_namespace(&self) -> AiResult<Option<String>> {
        self.0
            .embeddings_cache_namespace("openai-embeddings")
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
                .map(u32::try_from)
                .transpose()
                .map_err(|_| AiError::InvalidConfiguration("embeddings.dimensions"))?;
            let body = CreateEmbeddingRequest {
                model: prompt.model.into(),
                input: EmbeddingInput::StringArray(prompt.inputs.to_vec()),
                encoding_format: Some(EncodingFormat::Float),
                dimensions,
                ..Default::default()
            };
            let response = self
                .0
                .send(TextEndpoint::Embeddings, &body, false, Some(prompt.timeout))
                .await?;
            parse_embeddings(validate(Ok(response))?, self.name(), prompt)
        })
    }
}

fn parse_embeddings(
    response: Response,
    provider: &str,
    prompt: EmbeddingsPrompt<'_>,
) -> AiResult<EmbeddingsResponse> {
    #[derive(Deserialize)]
    struct Payload {
        data: Vec<Vector>,
        model: Option<String>,
        usage: Option<TokenUsage>,
    }

    #[derive(Deserialize)]
    struct Vector {
        index: usize,
        embedding: Vec<f32>,
    }

    #[derive(Deserialize)]
    struct TokenUsage {
        prompt_tokens: Option<u64>,
    }

    let payload: Payload = response.json().map_err(|_| AiError::InvalidResponse)?;
    if payload.data.len() != prompt.inputs.len() {
        return Err(AiError::InvalidResponse);
    }
    let dimensions = prompt.dimensions.unwrap_or(
        payload
            .data
            .first()
            .ok_or(AiError::InvalidResponse)?
            .embedding
            .len(),
    );
    if dimensions == 0 {
        return Err(AiError::InvalidResponse);
    }
    let mut vectors = vec![None; prompt.inputs.len()];
    for vector in payload.data {
        let slot = vectors
            .get_mut(vector.index)
            .ok_or(AiError::InvalidResponse)?;
        if slot.is_some()
            || vector.embedding.len() != dimensions
            || vector.embedding.iter().any(|value| !value.is_finite())
        {
            return Err(AiError::InvalidResponse);
        }
        *slot = Some(vector.embedding);
    }
    let embeddings = vectors
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or(AiError::InvalidResponse)?;
    Ok(EmbeddingsResponse {
        embeddings,
        usage: Usage {
            prompt_tokens: payload.usage.and_then(|usage| usage.prompt_tokens),
            completion_tokens: None,
        },
        provider: provider.into(),
        model: payload.model.unwrap_or_else(|| prompt.model.into()),
    })
}
