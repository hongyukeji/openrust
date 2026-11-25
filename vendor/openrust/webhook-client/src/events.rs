use crate::{WebhookConfig, WebhookRequest};
use std::sync::Arc;

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
