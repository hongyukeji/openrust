use openrust::http::client::Response;
use serde_json::Value;

use crate::{Result, events::WebhookTransportFailure, snapshot::Snapshot};

/// 应用代码在同一投递尝试中借用的数据，不进入事件队列或诊断序列化。
pub struct WebhookJobAttempt<'a> {
    pub scope: &'a str,
    pub delivery_id: &'a str,
    pub number: u32,
    pub outcome: &'static str,
    pub(crate) snapshot: &'a Snapshot,
    pub(crate) response: Option<&'a Response>,
    pub(crate) transport_failure: Option<WebhookTransportFailure>,
}

/// 应用钩子可以读取该次投递已保存的请求与重试设置。
#[non_exhaustive]
pub struct WebhookJobOptions<'a> {
    pub http_verb: &'a str,
    pub proxy: Option<&'a str>,
    pub maximum_tries: u32,
    pub retryable_status_codes: Option<&'a [u16]>,
    pub respect_retry_after: bool,
    pub retry_with_jitter: bool,
    pub timeout_in_seconds: u32,
    pub backoff_strategy: &'a str,
    pub signer: &'a str,
    pub signature_header_name: &'a str,
    pub timestamp_header_name: Option<&'a str>,
    pub verify_ssl: bool,
    pub throw_exception_on_failure: bool,
    pub queue: Option<&'a str>,
    pub connection: Option<&'a str>,
}

impl WebhookJobAttempt<'_> {
    pub fn message_id(&self) -> &str {
        &self.snapshot.uuid
    }
    pub fn get_webhook_url(&self) -> &str {
        &self.snapshot.signing_url
    }
    pub fn get_body(&self) -> Result<Vec<u8>> {
        self.snapshot.body_bytes()
    }
    pub fn get_response(&self) -> Option<&Response> {
        self.response
    }
    pub fn transport_failure(&self) -> Option<WebhookTransportFailure> {
        self.transport_failure
    }
    pub fn tags(&self) -> &[String] {
        &self.snapshot.tags
    }
    pub fn meta(&self) -> &Value {
        &self.snapshot.meta
    }

    /// 返回准备时选中的设置，不读取当前凭据或重新解析配置。
    pub fn options(&self) -> WebhookJobOptions<'_> {
        let snapshot = self.snapshot;
        WebhookJobOptions {
            http_verb: &snapshot.method,
            proxy: snapshot.proxy.as_deref(),
            maximum_tries: snapshot.tries,
            retryable_status_codes: snapshot.retryable.as_deref(),
            respect_retry_after: snapshot.respect_retry_after,
            retry_with_jitter: snapshot.jitter,
            timeout_in_seconds: snapshot.timeout,
            backoff_strategy: &snapshot.backoff,
            signer: &snapshot.signer,
            signature_header_name: &snapshot.signature_header,
            timestamp_header_name: snapshot.timestamp_header.as_deref(),
            verify_ssl: !snapshot.unverified_tls,
            throw_exception_on_failure: snapshot.throws_on_failure,
            queue: snapshot.queue.as_deref(),
            connection: snapshot.connection.as_deref(),
        }
    }

    /// 准备阶段的请求头名称。签名等发送时计算的头不包含在内。
    pub fn header_names(&self) -> impl Iterator<Item = &str> {
        self.snapshot.headers.iter().map(|(name, _)| name.as_str())
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
