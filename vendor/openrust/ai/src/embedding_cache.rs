use std::sync::Arc;

use openrust::{
    cache::{Cache, Repository, Ttl, Value as CacheValue},
    contracts::cache::Repository as _,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{AiError, AiResult, EmbeddingProvider, EmbeddingsPrompt, EmbeddingsResponse, Usage};

const DEFAULT_SECONDS: i64 = 60 * 60 * 24 * 30;

pub(crate) struct CacheOverride {
    pub(crate) seconds: Option<i64>,
    pub(crate) individually: Option<bool>,
}

#[derive(Deserialize)]
#[serde(default)]
struct Configuration {
    cache: bool,
    store: Option<String>,
    seconds: i64,
    individually: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            cache: false,
            store: None,
            seconds: DEFAULT_SECONDS,
            individually: true,
        }
    }
}

pub(crate) struct EmbeddingCache {
    repository: Arc<Repository>,
    ttl: Option<Ttl>,
    individually: bool,
    namespace: String,
}

impl EmbeddingCache {
    pub(crate) fn resolve(
        value: Option<Value>,
        options: Option<CacheOverride>,
        provider: &dyn EmbeddingProvider,
    ) -> AiResult<Option<Self>> {
        if options
            .as_ref()
            .is_some_and(|options| options.seconds.is_some_and(|seconds| seconds <= 0))
        {
            return Ok(None);
        }
        let config = if let Some(value) = value {
            serde_json::from_value::<Configuration>(value)
                .map_err(|_| AiError::InvalidConfiguration("caching.embeddings"))?
        } else {
            Configuration::default()
        };
        if options.is_none() && !config.cache {
            return Ok(None);
        }
        let seconds = options
            .as_ref()
            .and_then(|options| options.seconds)
            .unwrap_or(config.seconds);
        if seconds <= 0
            || config
                .store
                .as_ref()
                .is_some_and(|store| store.trim().is_empty())
        {
            return Err(AiError::InvalidConfiguration("caching.embeddings"));
        }
        let Some(namespace) = provider.embeddings_cache_namespace()? else {
            return Ok(None);
        };
        let repository = Cache::store(config.store.as_deref())
            .map_err(|_| AiError::EmbeddingCacheUnavailable)?;
        Ok(Some(Self {
            repository,
            ttl: Some(Ttl::Seconds(seconds)),
            individually: options
                .and_then(|options| options.individually)
                .unwrap_or(config.individually),
            namespace,
        }))
    }

    pub(crate) async fn generate(
        &self,
        provider: &dyn EmbeddingProvider,
        prompt: EmbeddingsPrompt<'_>,
    ) -> AiResult<EmbeddingsResponse> {
        if self.individually {
            return self.generate_individual(provider, prompt).await;
        }
        let key = self.key(prompt, "batch", prompt.inputs)?;
        let stored = self
            .repository
            .get(&key)
            .await
            .map_err(|_| AiError::EmbeddingCacheUnavailable)?;
        if let Some(entry) = decode::<Batch>(stored)
            && valid_batch(&entry.embeddings, prompt)
        {
            return Ok(EmbeddingsResponse {
                embeddings: entry.embeddings,
                usage: Usage::default(),
                provider: entry.provider,
                model: entry.model,
            });
        }
        let response = provider.embeddings(prompt).await?;
        if !valid_batch(&response.embeddings, prompt) {
            return Err(AiError::InvalidResponse);
        }
        let entry = BatchRef {
            embeddings: &response.embeddings,
            provider: &response.provider,
            model: &response.model,
        };
        let written = self.repository.put(&key, &encode(&entry)?, self.ttl).await;
        if !written.map_err(|_| AiError::EmbeddingCacheUnavailable)? {
            return Err(AiError::EmbeddingCacheUnavailable);
        }
        Ok(response)
    }

    async fn generate_individual(
        &self,
        provider: &dyn EmbeddingProvider,
        prompt: EmbeddingsPrompt<'_>,
    ) -> AiResult<EmbeddingsResponse> {
        let keys: Vec<_> = prompt
            .inputs
            .iter()
            .map(|input| self.key(prompt, "input", input))
            .collect::<AiResult<_>>()?;
        let addresses: Vec<_> = keys.iter().map(String::as_str).collect();
        let stored = self
            .repository
            .many(&addresses)
            .await
            .map_err(|_| AiError::EmbeddingCacheUnavailable)?;
        if stored.len() != addresses.len() {
            return Err(AiError::EmbeddingCacheUnavailable);
        }
        let mut embeddings: Vec<_> = stored
            .into_iter()
            .map(|entry| {
                decode::<Vec<f32>>(entry).filter(|vector| valid_vector(vector, prompt.dimensions))
            })
            .collect();
        let missing: Vec<_> = embeddings
            .iter()
            .enumerate()
            .filter_map(|(index, vector)| vector.is_none().then_some(index))
            .collect();
        if missing.is_empty() {
            let embeddings = collect(embeddings, prompt)?;
            return Ok(EmbeddingsResponse {
                embeddings,
                usage: Usage::default(),
                provider: provider.name().into(),
                model: prompt.model.into(),
            });
        }
        let inputs: Vec<_> = missing
            .iter()
            .map(|&index| prompt.inputs[index].clone())
            .collect();
        let miss_prompt = EmbeddingsPrompt {
            inputs: &inputs,
            ..prompt
        };
        let response = provider.embeddings(miss_prompt).await?;
        if !valid_batch(&response.embeddings, miss_prompt) {
            return Err(AiError::InvalidResponse);
        }
        let mut writes = Vec::with_capacity(missing.len());
        for (index, vector) in missing.into_iter().zip(response.embeddings) {
            writes.push((keys[index].clone(), encode(&vector)?));
            embeddings[index] = Some(vector);
        }
        let embeddings = collect(embeddings, prompt)?;
        let written = self.repository.put_many(&writes, self.ttl).await;
        if !written.map_err(|_| AiError::EmbeddingCacheUnavailable)? {
            return Err(AiError::EmbeddingCacheUnavailable);
        }
        Ok(EmbeddingsResponse {
            embeddings,
            usage: response.usage,
            provider: response.provider,
            model: response.model,
        })
    }

    fn key<T: Serialize + ?Sized>(
        &self,
        prompt: EmbeddingsPrompt<'_>,
        kind: &str,
        inputs: &T,
    ) -> AiResult<String> {
        let payload = serde_json::to_vec(&(
            &self.namespace,
            prompt.model,
            prompt.dimensions,
            kind,
            inputs,
        ))
        .map_err(|_| AiError::InvalidPrompt)?;
        Ok(format!("ai:embeddings:v1:{:x}", Sha256::digest(payload)))
    }
}

#[derive(Deserialize)]
struct Batch {
    embeddings: Vec<Vec<f32>>,
    provider: String,
    model: String,
}

#[derive(Serialize)]
struct BatchRef<'a> {
    embeddings: &'a [Vec<f32>],
    provider: &'a str,
    model: &'a str,
}

fn encode<T: Serialize>(value: &T) -> AiResult<CacheValue> {
    serde_json::to_value(value)
        .map(CacheValue::from)
        .map_err(|_| AiError::InvalidResponse)
}

fn decode<T: DeserializeOwned>(value: Option<CacheValue>) -> Option<T> {
    match value {
        Some(CacheValue::Json(value)) => serde_json::from_value(value).ok(),
        _ => None,
    }
}

fn collect(
    embeddings: Vec<Option<Vec<f32>>>,
    prompt: EmbeddingsPrompt<'_>,
) -> AiResult<Vec<Vec<f32>>> {
    let embeddings = embeddings
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or(AiError::InvalidResponse)?;
    if !valid_batch(&embeddings, prompt) {
        return Err(AiError::InvalidResponse);
    }
    Ok(embeddings)
}

fn valid_batch(embeddings: &[Vec<f32>], prompt: EmbeddingsPrompt<'_>) -> bool {
    embeddings.len() == prompt.inputs.len()
        && embeddings.first().is_some_and(|first| {
            let dimensions = prompt.dimensions.or(Some(first.len()));
            embeddings
                .iter()
                .all(|vector| valid_vector(vector, dimensions))
        })
}

fn valid_vector(vector: &[f32], dimensions: Option<usize>) -> bool {
    !vector.is_empty()
        && dimensions.is_none_or(|dimensions| dimensions == vector.len())
        && vector.iter().all(|value| value.is_finite())
}
