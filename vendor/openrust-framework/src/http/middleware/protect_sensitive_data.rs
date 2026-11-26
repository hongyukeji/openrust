use crate::{
    container::ParameterValue,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, RequestAttributes, Response},
};

const KEY: &str = "openrust.http.sensitive_data";

/// 为认证等敏感路由标记诊断边界；不修改实际请求、响应或认证行为。
pub struct ProtectSensitiveData;

impl ProtectSensitiveData {
    /// 当前请求的安全路由模板；离开请求作用域后返回 None。
    pub fn current_path() -> Option<String> {
        crate::http::request_context::with_attributes(Self::path).flatten()
    }

    /// 从请求快照读取标记，供请求结束后的诊断消费者使用。
    pub fn path(attributes: &RequestAttributes) -> Option<String> {
        attributes
            .get(KEY)?
            .downcast::<String>()
            .ok()
            .map(|value| (*value).clone())
    }

    pub(crate) fn mark(request: &Request, path: &str) {
        if Self::path(request.attributes()).is_none() {
            request
                .attributes()
                .insert(KEY, ParameterValue::new(path.to_owned()));
        }
    }

    pub(crate) fn clear(request: &Request) {
        request.attributes().remove(KEY);
    }
}

impl Middleware for ProtectSensitiveData {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            Self::mark(request, "[sensitive request]");
            Ok(next.run(request).await)
        })
    }
}
