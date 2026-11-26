use crate::{WebhookConfig, WebhookRequest};
use std::sync::Arc;
use std::time::Duration;

/// 接收流程的固定分类，可用作低基数指标标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReceiptOutcome {
    Accepted,
    Duplicate,
    Conflict,
    Ignored,
    UnverifiedHint,
    InvalidSignature,
    Rejected,
    RateLimited,
    Failed,
}

/// 接收流程完成后的观测数据。仅包含分类及耗时。
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct WebhookReceiptObserved {
    pub outcome: ReceiptOutcome,
    pub elapsed: Duration,
}

impl WebhookReceiptObserved {
    pub const NAME: &'static str = "openrust_webhook_client::events::WebhookReceiptObserved";
}

impl openrust::contracts::events::Event for WebhookReceiptObserved {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// 与上游相同的请求和配置载荷；只交给应用内监听器，不自动序列化敏感数据。
pub struct InvalidWebhookSignatureEvent {
    pub request: Arc<WebhookRequest>,
    pub config: Arc<WebhookConfig>,
}
impl openrust::contracts::events::Event for InvalidWebhookSignatureEvent {
    fn name(&self) -> &str {
        "openrust_webhook_client::events::InvalidWebhookSignatureEvent"
    }
}
