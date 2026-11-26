use openrust::http::HttpFuture;
use serde::Deserialize;
use serde_json::json;

use crate::{
    AiError, AiResult, RankedDocument, RerankingPrompt, RerankingProvider, RerankingResponse,
    Usage, reranking::validate_results,
};

use super::BedrockProvider;

impl RerankingProvider for BedrockProvider {
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
            prompt.validate()?;
            let mut request = json!({"query": prompt.query, "documents": prompt.documents});
            if let Some(limit) = prompt.limit {
                request["top_n"] = limit.min(prompt.documents.len()).into();
            }
            if prompt.model.starts_with("cohere.") {
                request["api_version"] = 2.into();
            }
            let (payload, _) = self
                .invoke_model(prompt.model, prompt.timeout, request)
                .await?;
            let results = Vec::<RankedDocument>::deserialize(&payload["results"])
                .map_err(|_| AiError::InvalidResponse)?;
            validate_results(&results, prompt.documents.len(), prompt.limit)?;
            Ok(RerankingResponse {
                results,
                usage: Usage::default(),
                provider: self.connection.name().into(),
                model: prompt.model.into(),
            })
        })
    }
}
