//! 从运营配置的固定上游读取文章摘要；请求方不能选择出站目标。

use std::time::Duration;

use openrust::http::axum::http::StatusCode;
use openrust::http::client::{HttpClient, HttpClientConfig};
use openrust::{Error, Result};

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ArticleSummary {
    pub(crate) summary: String,
}

pub(crate) struct ArticleSummaryService {
    origin: url::Url,
    client: HttpClient,
}

impl ArticleSummaryService {
    pub(crate) fn new(origin: &str) -> Result<Self> {
        let origin = url::Url::parse(origin).map_err(|_| upstream_failed())?;
        let client = HttpClient::new(HttpClientConfig {
            connect_timeout: Duration::from_secs(2),
            timeout: Duration::from_secs(5),
            max_response_bytes: 16 * 1024,
            proxy: None,
        })
        .map_err(|_| upstream_failed())?;
        Ok(Self { origin, client })
    }

    pub(crate) async fn fetch(&self, article_id: i64) -> Result<ArticleSummary> {
        let target = self
            .origin
            .join(&format!("articles/{article_id}/summary"))
            .map_err(|_| upstream_failed())?;
        let response = self
            .client
            .get(target.as_str())
            .send()
            .await
            .map_err(|_| upstream_failed())?;
        if response.status() != StatusCode::OK {
            return Err(upstream_failed());
        }
        let summary: ArticleSummary = response.json().await.map_err(|_| upstream_failed())?;
        if summary.summary.is_empty() || summary.summary.chars().count() > 500 {
            return Err(upstream_failed());
        }
        Ok(summary)
    }
}

fn upstream_failed() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_GATEWAY,
        "example_article_summary_upstream_failed",
        "文章摘要上游不可用",
    )
}
