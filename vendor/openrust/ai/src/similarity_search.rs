use openrust::{
    database::eloquent::{Builder, Model},
    http::HttpFuture,
    json_schema::{JsonSchemaTypeFactory, types::ObjectType},
    support::Collection,
};
use serde_json::Value;

use crate::{AiError, AiResult, RerankingOptions, Reranks, Tool, ToolRequest};

type Search = dyn for<'a> Fn(&'a str) -> HttpFuture<'a, AiResult<Vec<Value>>> + Send + Sync;
type Ranking = dyn Fn(&Value) -> String + Send + Sync;

/// 使用查询文本检索相似文档，必要时用 SDK 的 Reranking 重排。
pub struct SimilaritySearch {
    search: Box<Search>,
    description: String,
    ranking: Option<(Box<Ranking>, Option<usize>)>,
}

impl SimilaritySearch {
    pub fn using(
        search: impl for<'a> Fn(&'a str) -> HttpFuture<'a, AiResult<Vec<Value>>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            search: Box::new(search),
            description: "Search for documents similar to a given query.".into(),
            ranking: None,
        }
    }

    pub fn using_model<M: Model>(
        column: impl Into<String>,
        min_similarity: f64,
        limit: u64,
    ) -> AiResult<Self> {
        Self::using_model_with::<M>(column, min_similarity, limit, |query| query)
    }

    pub fn using_model_with<M: Model>(
        column: impl Into<String>,
        min_similarity: f64,
        limit: u64,
        query: impl Fn(Builder<'static, M>) -> Builder<'static, M> + Send + Sync + 'static,
    ) -> AiResult<Self> {
        let column = column.into();
        if column.trim().is_empty() || !(-1.0..=1.0).contains(&min_similarity) {
            return Err(AiError::InvalidConfiguration("similarity_search"));
        }
        let query = std::sync::Arc::new(query);
        Ok(Self::using(move |text| {
            let column = column.clone();
            let query = query.clone();
            Box::pin(async move {
                let base = M::query().map_err(|_| AiError::SimilaritySearchFailed)?;
                let mut builder = query(
                    base.where_vector_similar_to_text(column.as_str(), text, min_similarity, true)
                        .await
                        .map_err(|_| AiError::SimilaritySearchFailed)?,
                );
                if limit > 0 {
                    builder = builder.limit(limit);
                }
                let records = builder
                    .get()
                    .await
                    .map_err(|_| AiError::SimilaritySearchFailed)?;
                let field = column.rsplit('.').next().unwrap_or(&column);
                records
                    .iter()
                    .map(|record| {
                        let mut row = record
                            .to_array()
                            .map_err(|_| AiError::SimilaritySearchFailed)?;
                        row.remove(field);
                        Ok(Value::Object(row))
                    })
                    .collect()
            })
        }))
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn rerank(
        mut self,
        by: impl Fn(&Value) -> String + Send + Sync + 'static,
        limit: Option<usize>,
    ) -> Self {
        self.ranking = Some((Box::new(by), limit));
        self
    }
}

impl Tool for SimilaritySearch {
    fn name(&self) -> &str {
        "SimilaritySearch"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType {
        schema.object([(
            "query",
            schema
                .string()
                .description("The search query.")
                .required(true)
                .into(),
        )])
    }

    fn handle<'a>(&'a self, request: ToolRequest<'a>) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            let query = request
                .get("query")
                .and_then(Value::as_str)
                .filter(|query| !query.trim().is_empty())
                .ok_or(AiError::InvalidToolArguments)?;
            let results = (self.search)(query).await?;
            if results.is_empty() {
                return Ok("No relevant results found.".into());
            }
            let results = Collection::make(results);
            let results = if let Some((by, limit)) = &self.ranking {
                results
                    .rerank_with(
                        by,
                        query,
                        RerankingOptions {
                            limit: *limit,
                            ..Default::default()
                        },
                    )
                    .await?
            } else {
                results
            };
            let body = serde_json::to_string_pretty(&results.all().values().collect::<Vec<_>>())
                .map_err(|_| AiError::InvalidResponse)?;
            Ok(format!(
                "Relevant results found. They are listed below in order of relevance:\n\n{body}"
            ))
        })
    }
}
