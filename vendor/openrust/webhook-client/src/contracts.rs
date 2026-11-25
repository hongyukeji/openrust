use crate::{Error, Result, WebhookConfig};
use http::{HeaderMap, Method, Uri};
use openrust::http::{Response, StatusCode};

/// 未重新编码的请求快照。创建快照前由 HTTP 入口执行流读取预算。
#[derive(Clone)]
pub struct WebhookRequest {
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Vec<u8>,
}
impl WebhookRequest {
    pub fn new(method: Method, headers: HeaderMap, body: Vec<u8>) -> Result<Self> {
        if body.len() > 16 * 1024 * 1024 {
            return Err(Error::PayloadTooLarge);
        }
        let size = headers.iter().try_fold(0usize, |size, (name, value)| {
            size.checked_add(name.as_str().len())
                .and_then(|size| size.checked_add(value.len()))
        });
        if headers.len() > 128 || size.is_none_or(|size| size > 64 * 1024) {
            return Err(Error::HeadersTooLarge);
        }
        Ok(Self {
            uri: Uri::from_static("/"),
            method,
            headers,
            body,
        })
    }
    /// 保留原请求的目标与查询串，供供应商自定义签名使用。
    pub fn with_uri(mut self, uri: Uri) -> Self {
        self.uri = uri;
        self
    }
    pub fn uri(&self) -> &Uri {
        &self.uri
    }
    pub fn method(&self) -> &Method {
        &self.method
    }
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}
/// 接收签名验证。错误与验证失败分开，不能将错误降级为未签名提示。
pub trait SignatureValidator: Send + Sync {
    fn is_valid(&self, request: &WebhookRequest, config: &WebhookConfig) -> Result<bool>;
}
pub trait WebhookProfile: Send + Sync {
    fn should_process(&self, request: &WebhookRequest, config: &WebhookConfig) -> Result<bool>;
}
pub struct ProcessEverythingWebhookProfile;
impl WebhookProfile for ProcessEverythingWebhookProfile {
    fn should_process(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
        Ok(true)
    }
}
pub trait RespondsToWebhook: Send + Sync {
    fn respond_to_valid_webhook(
        &self,
        request: &WebhookRequest,
        config: &WebhookConfig,
    ) -> Result<Response>;
}
pub struct DefaultRespondsTo;
impl RespondsToWebhook for DefaultRespondsTo {
    fn respond_to_valid_webhook(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<Response> {
        Response::json(StatusCode::OK, serde_json::json!({"message":"ok"})).map_err(|source| {
            Error::Runtime {
                source: Box::new(source),
            }
        })
    }
}
