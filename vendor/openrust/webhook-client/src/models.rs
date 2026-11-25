use crate::{Result, VerifiedWebhook, WebhookStore};
use http::HeaderMap;
use serde_json::Value;

/// 持久接收记录。查询及写入委托存储适配，不在模型中编排通知流程。
/// 记录可由自定义适配器构造，本身不代表通过验签；只有 VerifiedWebhook 表达验证结果。
/// 不实现 Debug／Serialize，避免正文和请求头自动进入诊断或队列载荷。
#[derive(Clone)]
pub struct WebhookCall {
    pub id: String,
    pub scope: String,
    pub name: String,
    pub revision: String,
    pub url: String,
    pub method: http::Method,
    pub headers: HeaderMap,
    pub raw_body: Vec<u8>,
    pub created_at: i64,
    pub exception: Option<Value>,
}
impl WebhookCall {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn scope(&self) -> &str {
        &self.scope
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn method(&self) -> &http::Method {
        &self.method
    }
    pub fn raw_body(&self) -> &[u8] {
        &self.raw_body
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn exception(&self) -> Option<&Value> {
        self.exception.as_ref()
    }
    pub fn header_bag(&self) -> &HeaderMap {
        &self.headers
    }
    pub fn headers(&self) -> &HeaderMap {
        self.header_bag()
    }
    pub fn headers_to_store(
        config: &crate::WebhookConfig,
        request: &crate::WebhookRequest,
    ) -> Result<HeaderMap> {
        config.headers_to_store(request.headers())
    }
    pub async fn store_webhook(
        store: &dyn WebhookStore,
        webhook: &VerifiedWebhook<'_>,
    ) -> Result<Self> {
        store.store_webhook(webhook).await
    }
    pub async fn find(store: &dyn WebhookStore, id: &str) -> Result<Option<Self>> {
        store.find(id).await
    }
    /// 返回当前存储范围内一个有限批次的可清理记录 ID。
    pub async fn prunable(store: &dyn WebhookStore, days: u32, limit: u32) -> Result<Vec<String>> {
        store.prunable(days, limit).await
    }
    /// 已保存的输入；升级前的原文记录没有解析快照。
    pub async fn payload(&self, store: &dyn WebhookStore) -> Result<Option<Value>> {
        store.payload(self.id()).await
    }

    /// 从当前范围的持久原文读取附件，返回框架原生上传对象。
    pub async fn get_attachments(
        &self,
        store: &dyn WebhookStore,
    ) -> Result<openrust::http::UploadedFileValue> {
        store.get_attachments(self.id()).await
    }

    pub async fn clear_exception(&mut self, store: &dyn WebhookStore) -> Result<&mut Self> {
        store.set_exception(self, None).await?;
        self.exception = None;
        Ok(self)
    }
    /// 诊断须由调用者先脱敏，存储适配还会限制 JSON 字节数。
    pub async fn save_exception(
        &mut self,
        store: &dyn WebhookStore,
        exception: Value,
    ) -> Result<&mut Self> {
        store.set_exception(self, Some(&exception)).await?;
        self.exception = Some(exception);
        Ok(self)
    }
}
