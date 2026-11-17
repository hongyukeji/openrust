//! Application-scoped providers for turning query text into a numeric vector.

use std::{fmt::Write as _, time::Duration};

use serde::Deserialize;
use serde_json::json;

use crate::{
    cache::{Cache, Ttl, Value},
    contracts::{cache::Repository as _, container::Container as _},
    database::{Connection, DatabaseError, DatabaseFuture, DatabaseResult},
    support::facades::{Http, application_scope},
};

/// Generates an embedding with an application-selected model.
pub trait EmbeddingProvider: Send + Sync + 'static {
    /// Identifies the provider, model, dimensions, and any account-specific settings.
    fn cache_identity(&self) -> &str;

    fn embed<'a>(&'a self, text: &'a str) -> DatabaseFuture<'a, DatabaseResult<Vec<f64>>>;
}

/// Register this service in the application container for text vector queries.
pub struct Embeddings {
    provider: Box<dyn EmbeddingProvider>,
}

impl Embeddings {
    pub fn new(provider: impl EmbeddingProvider) -> Self {
        Self {
            provider: Box::new(provider),
        }
    }

    async fn cached(&self, text: &str) -> DatabaseResult<Vec<f64>> {
        let identity = self.provider.cache_identity();
        let key = format!(
            "embeddings:v1:{}",
            digest(&format!("{}:{identity}{text}", identity.len()))
        );
        let cache = Cache::store(None).map_err(embedding_error)?;
        if let Some(Value::Json(value)) = cache.get(&key).await.map_err(embedding_error)?
            && let Ok(vector) = serde_json::from_value::<Vec<f64>>(value)
            && valid(&vector)
        {
            return Ok(vector);
        }
        let vector = self.provider.embed(text).await?;
        if !valid(&vector) {
            return Err(DatabaseError::InvalidQuery(
                "embedding must contain finite values",
            ));
        }
        cache
            .put(
                &key,
                &Value::Json(json!(vector)),
                Some(Ttl::Seconds(30 * 24 * 60 * 60)),
            )
            .await
            .map_err(embedding_error)?;
        Ok(vector)
    }
}

/// An adapter for an HTTP service implementing the OpenAI embeddings protocol.
pub struct HttpEmbeddingProvider {
    endpoint: url::Url,
    model: String,
    token: String,
    dimensions: Option<u32>,
    identity: String,
}

impl HttpEmbeddingProvider {
    /// The endpoint is the complete embeddings URL and belongs in trusted application configuration.
    pub fn new(
        endpoint: &str,
        model: impl Into<String>,
        token: impl Into<String>,
        dimensions: Option<u32>,
    ) -> DatabaseResult<Self> {
        let endpoint = url::Url::parse(endpoint)
            .map_err(|_| DatabaseError::InvalidConfiguration("embedding endpoint"))?;
        let model = model.into();
        let token = token.into();
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
            || model.trim().is_empty()
            || dimensions == Some(0)
        {
            return Err(DatabaseError::InvalidConfiguration("embedding provider"));
        }
        let identity =
            digest(&json!([endpoint.as_str(), model, dimensions, digest(&token)]).to_string());
        Ok(Self {
            endpoint,
            model,
            token,
            dimensions,
            identity,
        })
    }
}

impl EmbeddingProvider for HttpEmbeddingProvider {
    fn cache_identity(&self) -> &str {
        &self.identity
    }

    fn embed<'a>(&'a self, text: &'a str) -> DatabaseFuture<'a, DatabaseResult<Vec<f64>>> {
        Box::pin(async move {
            #[derive(Deserialize)]
            struct Embedding {
                index: usize,
                embedding: Vec<f64>,
            }

            #[derive(Deserialize)]
            struct Response {
                data: Vec<Embedding>,
            }

            let mut body = json!({"model": self.model, "input": text, "encoding_format": "float"});
            if let Some(dimensions) = self.dimensions {
                body["dimensions"] = json!(dimensions);
            }
            let mut request = Http::acceptJson()
                .map_err(embedding_error)?
                .timeout(Duration::from_secs(30))
                .withoutRedirecting();
            if !self.token.is_empty() {
                request = request.withToken(&self.token);
            }
            let response = request
                .post(self.endpoint.as_str(), &body)
                .await
                .map_err(embedding_error)?;
            if !response.successful() {
                return Err(embedding_error(std::io::Error::other(format!(
                    "embedding endpoint returned HTTP {}",
                    response.status().as_u16()
                ))));
            }
            let Response { mut data } = response.json::<Response>().map_err(embedding_error)?;
            if data.len() != 1 || data[0].index != 0 {
                return Err(DatabaseError::InvalidQuery("embedding response"));
            }
            let result = data.remove(0).embedding;
            if self
                .dimensions
                .is_some_and(|size| size as usize != result.len())
            {
                return Err(DatabaseError::InvalidQuery("embedding response dimensions"));
            }
            Ok(result)
        })
    }
}

pub(super) async fn cached_for(connection: Connection, text: &str) -> DatabaseResult<Vec<f64>> {
    let owner = Connection::model_container(Some(&connection))
        .or_else(|| Connection::model_container(None))
        .ok_or(DatabaseError::UnsupportedOperation(
            "text embeddings require an application",
        ))?;
    application_scope(owner.clone(), async move {
        owner
            .makeAsync::<Embeddings>()
            .await
            .map_err(embedding_error)?
            .cached(text)
            .await
    })
    .await
}

pub(super) fn valid(vector: &[f64]) -> bool {
    !vector.is_empty() && vector.iter().all(|value| value.is_finite())
}

fn embedding_error(error: impl std::error::Error + Send + Sync + 'static) -> DatabaseError {
    DatabaseError::Embedding(Box::new(error))
}

fn digest(input: &str) -> String {
    let mut output = String::with_capacity(64);
    for byte in aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, input.as_bytes()).as_ref() {
        write!(output, "{byte:02x}").expect("writing to String");
    }
    output
}
