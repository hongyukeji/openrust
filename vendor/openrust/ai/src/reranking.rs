use std::{collections::HashSet, sync::Arc, time::Duration};

use openrust::{http::HttpFuture, support::Collection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AgentResponse, Ai, AiError, AiResult, FakeRerankingGateway, FakeRerankingResult,
    RecordedReranking, TextPrompt, TextProvider, Usage,
    connection::Connection,
    text::{transport_error, validate},
};

pub struct Reranking;

impl Reranking {
    pub fn of(
        documents: impl IntoIterator<Item = impl Into<String>>,
    ) -> AiResult<PendingReranking> {
        let documents: Vec<_> = documents.into_iter().map(Into::into).collect();
        if documents.is_empty() || documents.iter().any(|document| document.trim().is_empty()) {
            return Err(AiError::InvalidPrompt);
        }
        Ok(PendingReranking {
            documents,
            limit: None,
            timeout: Duration::from_secs(30),
        })
    }
    pub fn fake(
        responses: impl IntoIterator<Item = FakeRerankingResult>,
    ) -> AiResult<Arc<FakeRerankingGateway>> {
        Ai::fake_reranking(responses)
    }

    pub fn fake_with(
        resolve: impl Fn(&RecordedReranking) -> AiResult<FakeRerankingResult> + Send + Sync + 'static,
    ) -> AiResult<Arc<FakeRerankingGateway>> {
        Ok(Ai::manager()?.fake_reranking_with(resolve))
    }

    pub fn is_faked() -> AiResult<bool> {
        Ai::reranking_is_faked()
    }

    #[track_caller]
    pub fn assert_reranked(predicate: impl Fn(&RecordedReranking) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_reranked(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_reranked(predicate: impl Fn(&RecordedReranking) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_reranked(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_reranked() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_reranked();
        Ok(())
    }

    fn fake_gateway() -> AiResult<Arc<FakeRerankingGateway>> {
        Ai::manager()?.reranking_fake().ok_or(AiError::Unavailable)
    }
}

pub struct PendingReranking {
    documents: Vec<String>,
    limit: Option<usize>,
    timeout: Duration,
}

impl PendingReranking {
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn rerank(self, query: &str) -> AiResult<RerankingResponse> {
        self.rerank_with(query, None, None).await
    }

    pub async fn rerank_with(
        self,
        query: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<RerankingResponse> {
        if query.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        if self.limit == Some(0) || self.timeout.is_zero() {
            return Err(AiError::InvalidConfiguration(
                "reranking 返回数和超时必须大于零",
            ));
        }
        let manager = Ai::manager()?;
        if model.is_some_and(|name| name.trim().is_empty()) {
            return Err(AiError::MissingModel);
        }
        let limit = self.limit.map(|limit| limit.min(self.documents.len()));
        if let Some(fake) = manager.reranking_fake() {
            let model = model.map(str::to_owned).or_else(|| {
                manager
                    .reranking_provider(provider)
                    .ok()
                    .and_then(|provider| provider.default_reranking_model().ok().map(str::to_owned))
            });
            return fake.respond(RecordedReranking {
                documents: self.documents,
                query: query.into(),
                limit,
                timeout: self.timeout,
                provider: manager.reranking_provider_name(provider).ok(),
                model,
            });
        }
        let provider = manager.reranking_provider(provider)?;
        let model = match model {
            Some(model) => model,
            None => provider.default_reranking_model()?,
        };
        let response = provider
            .rerank(RerankingPrompt {
                documents: &self.documents,
                query,
                model,
                limit,
                timeout: self.timeout,
            })
            .await?;
        validate_results(&response.results, self.documents.len(), limit)?;
        Ok(response)
    }
}

pub struct RerankingPrompt<'a> {
    pub documents: &'a [String],
    pub query: &'a str,
    pub model: &'a str,
    pub limit: Option<usize>,
    pub timeout: Duration,
}

/// 索引基于原始文档列表，顺序与上游返回一致。
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RankedDocument {
    pub index: usize,
    pub relevance_score: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RerankingResponse {
    pub results: Vec<RankedDocument>,
    pub usage: Usage,
    pub provider: String,
    pub model: String,
}

pub trait RerankingProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_reranking_model(&self) -> AiResult<&str>;
    fn rerank<'a>(
        &'a self,
        prompt: RerankingPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<RerankingResponse>>;
}

#[derive(Clone, Copy)]
pub(crate) enum RerankingProtocol {
    TopN,
    Voyage,
}

#[derive(Clone)]
pub(crate) struct HttpRerankingProvider {
    pub(crate) connection: Arc<Connection>,
    pub(crate) protocol: RerankingProtocol,
}

impl TextProvider for HttpRerankingProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        Err(AiError::EndpointNotSupported)
    }

    fn generate_text<'a>(&'a self, _: TextPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn reranking_provider(&self) -> Option<Arc<dyn RerankingProvider>> {
        Some(Arc::new(self.clone()))
    }
}

impl RerankingProvider for HttpRerankingProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_reranking_model(&self) -> AiResult<&str> {
        self.connection.default_reranking_model()
    }

    fn rerank<'a>(
        &'a self,
        prompt: RerankingPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<RerankingResponse>> {
        Box::pin(async move {
            if prompt.documents.is_empty()
                || prompt.query.trim().is_empty()
                || prompt
                    .documents
                    .iter()
                    .any(|document| document.trim().is_empty())
            {
                return Err(AiError::InvalidPrompt);
            }
            if prompt.model.trim().is_empty() {
                return Err(AiError::MissingModel);
            }
            if prompt.limit == Some(0) || prompt.timeout.is_zero() {
                return Err(AiError::InvalidConfiguration(
                    "reranking 返回数和超时必须大于零",
                ));
            }
            let mut body =
                json!({"model":prompt.model,"query":prompt.query,"documents":prompt.documents});
            let (limit_field, response_field) = match self.protocol {
                RerankingProtocol::TopN => ("top_n", "results"),
                RerankingProtocol::Voyage => ("top_k", "data"),
            };
            if let Some(limit) = prompt.limit {
                body[limit_field] = limit.min(prompt.documents.len()).into();
            }
            let response = self
                .connection
                .request(Some(prompt.timeout))?
                .post("rerank", &body)
                .await
                .map_err(transport_error)?;
            let mut data: Value = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let results: Vec<RankedDocument> = serde_json::from_value(data[response_field].take())
                .map_err(|_| AiError::InvalidResponse)?;
            validate_results(&results, prompt.documents.len(), prompt.limit)?;
            let prompt_tokens = data["usage"]["total_tokens"]
                .as_u64()
                .or_else(|| data["meta"]["billed_units"]["input_tokens"].as_u64());
            Ok(RerankingResponse {
                results,
                usage: Usage {
                    prompt_tokens,
                    completion_tokens: None,
                },
                provider: self.connection.name().into(),
                model: data["model"].as_str().unwrap_or(prompt.model).into(),
            })
        })
    }
}

pub(crate) fn validate_results(
    results: &[RankedDocument],
    documents: usize,
    limit: Option<usize>,
) -> AiResult<()> {
    let mut indexes = HashSet::new();
    let maximum = limit.unwrap_or(documents).min(documents);
    if results.len() > maximum
        || results.iter().any(|result| {
            result.index >= documents
                || !result.relevance_score.is_finite()
                || !indexes.insert(result.index)
        })
    {
        return Err(AiError::InvalidResponse);
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
pub struct RerankingOptions<'a> {
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub limit: Option<usize>,
    pub timeout: Option<Duration>,
}

/// 从集合中的条目提取文本，按模型返回的索引对原条目重新排序。
pub trait Reranks<T: Clone + Sync>: Sized {
    fn rerank<'a>(
        &'a self,
        by: impl FnMut(&T) -> String,
        query: &'a str,
    ) -> HttpFuture<'a, AiResult<Self>> {
        self.rerank_with(by, query, RerankingOptions::default())
    }

    fn rerank_with<'a>(
        &'a self,
        by: impl FnMut(&T) -> String,
        query: &'a str,
        options: RerankingOptions<'a>,
    ) -> HttpFuture<'a, AiResult<Self>>;
}

impl<T: Clone + Sync> Reranks<T> for Collection<T> {
    fn rerank_with<'a>(
        &'a self,
        by: impl FnMut(&T) -> String,
        query: &'a str,
        options: RerankingOptions<'a>,
    ) -> HttpFuture<'a, AiResult<Self>> {
        let items: Vec<_> = self.all().values().collect();
        let pending = Reranking::of(items.iter().copied().map(by));
        Box::pin(async move {
            let mut pending = pending?;
            if let Some(limit) = options.limit {
                pending = pending.limit(limit);
            }
            if let Some(timeout) = options.timeout {
                pending = pending.timeout(timeout);
            }
            let result = pending
                .rerank_with(query, options.provider, options.model)
                .await?;
            let ranked = result
                .results
                .into_iter()
                .map(|result| items[result.index].clone());
            Ok(Collection::make(ranked))
        })
    }
}
