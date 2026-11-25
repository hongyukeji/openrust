use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::http::client::Response;
use serde_json::Value;

use crate::{Error, Result, snapshot::Snapshot};

/// 应用代码在同一投递尝试中借用的数据，不进入事件队列或诊断序列化。
pub struct WebhookJobAttempt<'a> {
    pub scope: &'a str,
    pub delivery_id: &'a str,
    pub number: u32,
    pub outcome: &'static str,
    pub(crate) snapshot: &'a Snapshot,
    pub(crate) response: Option<&'a Response>,
}

impl WebhookJobAttempt<'_> {
    pub fn message_id(&self) -> &str {
        &self.snapshot.uuid
    }
    pub fn get_webhook_url(&self) -> &str {
        &self.snapshot.signing_url
    }
    pub fn get_body(&self) -> Result<Vec<u8>> {
        STANDARD
            .decode(&self.snapshot.body)
            .map_err(|_| Error::InvalidRecord)
    }
    pub fn get_response(&self) -> Option<&Response> {
        self.response
    }
    pub fn tags(&self) -> &[String] {
        &self.snapshot.tags
    }
    pub fn meta(&self) -> &Value {
        &self.snapshot.meta
    }
}

/// 为同名发送 Job 自定义停止重试及终态失败处理。
/// 这两个同步钩子在当前进程中运行，应快速返回。
pub trait CallWebhookJob: Send + Sync {
    fn should_be_removed_from_queue(&self, _: &WebhookJobAttempt<'_>) -> bool {
        false
    }

    fn failed(&self, _: &WebhookJobAttempt<'_>) -> Result<()> {
        Ok(())
    }
}
