//! 框架错误与统一 JSON 响应封装。

use std::collections::BTreeMap;

use crate::config::HttpStatusMode;
use axum_core::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use serde::Serialize;
use std::future::Future;
use std::sync::Arc;

use crate::config::ConfigError;

/// 由框架类型信息构造的服务身份；不可从部署值或任意来源字符串创建。
///
/// ```compile_fail
/// use openrust::core::error::ServiceIdentity;
/// let _ = ServiceIdentity { name: "credential_password" };
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ServiceIdentity {
    name: &'static str,
}

impl ServiceIdentity {
    /// 已注册服务的 Rust 类型名。
    pub fn name(self) -> &'static str {
        self.name
    }

    pub(crate) fn from_type_name(name: &'static str) -> Self {
        Self { name }
    }
}

impl std::fmt::Display for ServiceIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name)
    }
}

/// 框架和生成应用通用的 Result 别名。
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// 按字段路径索引的校验错误（例如 `user.email`、`items[2].qty`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ValidationErrors(BTreeMap<String, Vec<String>>);

impl ValidationErrors {
    /// 创建空集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录 `field` 的失败消息。
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.0.entry(field.into()).or_default().push(message.into());
    }

    /// 尚未记录任何错误时返回 `true`。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 按字段顺序遍历 `(field, messages)`。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}

/// 框架统一使用的错误类型。
///
/// 处理器、命令或插件可能遇到的所有错误都通过此类型表示，因此可以统一使用 `?` 传播错误，
/// 并由唯一位置负责生成 HTTP 响应。客户端错误包含可安全展示给调用方的消息；
/// [`Error::Internal`] 会隐藏错误来源，只写入日志。
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// 请求格式错误：正文不可读或参数无效。
    #[error("{0}")]
    BadRequest(String),
    /// 调用方未通过身份认证。
    #[error("authentication required")]
    Unauthorized,
    /// 调用方已通过身份认证，但无权执行此操作。
    #[error("{0}")]
    Forbidden(String),
    /// 请求的资源不存在。
    #[error("{0}")]
    NotFound(String),
    /// 请求与当前状态冲突（例如键重复或版本过期）。
    #[error("{0}")]
    Conflict(String),
    /// 输入格式正确，但未通过校验规则。
    #[error("validation failed")]
    Validation(ValidationErrors),
    /// 已解析的输入未通过结构校验，并附带可读原因。
    #[error("{detail}")]
    ValidationDetail {
        /// 解析失败的说明。
        detail: String,
        /// 解析器提供的字段路径。
        errors: ValidationErrors,
    },
    /// 调用方超出限流配额。
    #[error("too many requests")]
    RateLimited,
    /// 调用方超出限流配额；应在指定秒数后重试。
    #[error("too many requests")]
    RateLimitedWithRetryAfter(u64),
    /// 配置无法加载或无效；仅在引导启动期间发生。
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
    /// 应用自定义错误，可包含语义 HTTP 状态码和稳定错误码。
    #[error("{message}")]
    Custom {
        /// 响应使用的 HTTP 状态码。
        status: Option<StatusCode>,
        /// 供客户端分支处理的稳定 `snake_case` 错误码。
        code: String,
        /// 可安全展示的可读详情。
        message: String,
    },
    /// 带具体错误来源的 HTTP 异常；Handler 可按来源类型报告或渲染。
    #[error("{message}")]
    HttpWithSource {
        /// HTTP 状态码，包含非标准但有效的扩展状态码。
        status: StatusCode,
        /// 稳定的公开错误码。
        code: String,
        /// 安全的公开说明；5xx 在非调试模式下仍被隐藏。
        message: String,
        /// 具体错误来源，保留给按类型的 Handler 回调。
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// 未预期的失败。错误来源会写入日志；调用方只会看到通用消息。
    #[error("internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// 容器工厂失败；公开详情不包含底层配置值，source 保留原始错误链。
    #[error("service `{service}` initialization failed (cause: {})", source.initialization_cause())]
    ServiceInitialization {
        /// 构造失败的 Rust 服务类型。
        service: ServiceIdentity,
        /// 内部错误链；不得直接作为 HTTP 响应输出。
        #[source]
        source: std::sync::Arc<Error>,
    },
}

// Debug 常用于 tracing/断言，不能递归打印任意业务或配置 source 中的秘密值。
impl std::fmt::Debug for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut output = formatter.debug_struct("Error");
        output
            .field("code", &self.code())
            .field("status", &self.status());
        if let Self::ServiceInitialization { service, source } = self {
            output
                .field("service", service)
                .field("cause", &source.initialization_cause());
        }
        output.finish_non_exhaustive()
    }
}

impl Error {
    /// 从具体服务类型包装初始化失败；诊断身份不接受任意来源文本。
    ///
    /// ```compile_fail
    /// use openrust::core::Error;
    /// use std::sync::Arc;
    /// let _ = Error::ServiceInitialization {
    ///     service: "token-sentinel",
    ///     source: Arc::new(Error::custom("failure", "private-mail-body")),
    /// };
    /// ```
    pub fn service_initialization<T: ?Sized + 'static>(source: Error) -> Self {
        Self::ServiceInitialization {
            service: ServiceIdentity::from_type_name(std::any::type_name::<T>()),
            source: Arc::new(source),
        }
    }

    // 来源文本和自定义code可能包含部署值；只展示封闭类别或框架构造的诊断。
    fn initialization_cause(&self) -> String {
        let mut current = self;
        let mut output = String::new();
        for _ in 0..16 {
            match current {
                Self::ServiceInitialization { service, source } => {
                    output.push_str("dependency `");
                    output.push_str(service.name());
                    output.push_str("` -> ");
                    current = source;
                }
                Self::Config(ConfigError::Diagnostic(diagnostic)) => {
                    output.push_str(&diagnostic.to_string());
                    return output;
                }
                _ => {
                    output.push_str(match current {
                        Self::Config(ConfigError::Source(_)) => "configuration_source_error",
                        Self::Config(ConfigError::Dependency(_)) => {
                            "configuration_dependency_error"
                        }
                        Self::Config(_) => "configuration_error",
                        Self::Internal(_) => "internal_error",
                        Self::Custom { .. } | Self::HttpWithSource { .. } => "custom_error",
                        _ => current.code(),
                    });
                    return output;
                }
            }
        }
        output.push_str("cause_chain_truncated");
        output
    }

    /// 将任意错误包装为 [`Error::Internal`]。
    pub fn internal<E>(source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Internal(Box::new(source))
    }

    /// 创建包含稳定 `code` 的应用自定义错误。
    pub fn custom(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Custom {
            status: None,
            code: code.into(),
            message: message.into(),
        }
    }

    /// 创建带有明确语义 HTTP 状态码的应用自定义错误。
    pub fn custom_with_status(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Custom {
            status: Some(status),
            code: code.into(),
            message: message.into(),
        }
    }

    /// 从具体错误创建 HTTP 异常，保留来源类型供应用 Handler 分派。
    pub fn http_with_source<E>(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
        source: E,
    ) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::HttpWithSource {
            status,
            code: code.into(),
            message: message.into(),
            source: Box::new(source),
        }
    }

    /// 创建限流错误，并携带以秒为单位的 `Retry-After` 延迟。
    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self::RateLimitedWithRetryAfter(retry_after_secs)
    }

    /// 创建带有可读详情的结构校验错误。
    pub fn validation_with_detail(detail: impl Into<String>, errors: ValidationErrors) -> Self {
        Self::ValidationDetail {
            detail: detail.into(),
            errors,
        }
    }

    /// 此错误生成的 HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) | Self::ValidationDetail { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited | Self::RateLimitedWithRetryAfter(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Custom { status, .. } => status.unwrap_or(StatusCode::BAD_REQUEST),
            Self::HttpWithSource { status, .. } => *status,
            Self::Config(_) | Self::Internal(_) | Self::ServiceInitialization { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    /// 稳定且可供机器读取的错误码（`not_found`、`validation_failed` 等）。
    pub fn code(&self) -> &str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Validation(_) | Self::ValidationDetail { .. } => "validation_failed",
            Self::RateLimited | Self::RateLimitedWithRetryAfter(_) => "rate_limited",
            Self::Config(_) => "configuration_error",
            Self::Custom { code, .. } | Self::HttpWithSource { code, .. } => code,
            Self::Internal(_) => "internal_error",
            Self::ServiceInitialization { .. } => "service_initialization_failed",
        }
    }

    /// 对于详情不得泄露给调用方的 5xx 错误，返回 `true`。
    pub fn is_server_error(&self) -> bool {
        self.status().is_server_error()
    }

    /// 使用统一 JSON 封装生成错误响应，并隐藏服务器内部详情。
    pub fn to_api_response(&self) -> ApiResponse<()> {
        let fallback = if self.is_server_error() {
            "internal server error".to_owned()
        } else {
            self.to_string()
        };
        let message = if self.is_server_error() {
            fallback
        } else {
            let key = format!("openrust.error.{}", self.code());
            crate::i18n::translate_current(&key, &crate::i18n::MessageArgs::new())
                .filter(|translated| translated != &key)
                .unwrap_or(fallback)
        };
        let errors = match self {
            Self::Validation(errors) => Some(errors.clone()),
            Self::ValidationDetail { errors, .. } if !errors.is_empty() => Some(errors.clone()),
            _ => None,
        };
        ApiResponse::failure(self.code(), message, self.status(), errors)
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Self::internal(source)
    }
}

impl From<ValidationErrors> for Error {
    fn from(errors: ValidationErrors) -> Self {
        Self::Validation(errors)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let context = response_context();
        if let (Some(handler), Some(request)) =
            (&context.exception_handler, &context.exception_request)
        {
            return handler.render_for_request(self, request);
        }
        if self.is_server_error() && should_log_server_error(&self) {
            tracing::error!(code = self.code(), status = %self.status(), "request failed");
        } else if !self.is_server_error() {
            tracing::debug!(code = self.code(), status = %self.status(), "request rejected");
        }
        let mut response = self.to_api_response().into_response();
        if let Self::RateLimitedWithRetryAfter(seconds) = self
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

fn should_log_server_error(error: &Error) -> bool {
    error.code() != "http_overloaded"
}

/// HTTP middleware 为每个请求设置的响应上下文。
#[derive(Clone)]
pub struct ResponseContext {
    /// 在响应头和正文中保持一致的关联 ID。
    pub request_id: String,
    /// 已配置的 HTTP 状态码策略。
    pub http_status: HttpStatusMode,
    /// 当前应用的异常策略；命令和独立响应没有请求策略。
    pub(crate) exception_handler: Option<Arc<super::ExceptionHandler>>,
    /// 当前 HTTP 请求的内容协商快照。
    pub(crate) exception_request: Option<super::ExceptionRequest>,
}

impl ResponseContext {
    /// 创建不带 HTTP 异常策略的独立响应上下文。
    pub fn new(request_id: impl Into<String>, http_status: HttpStatusMode) -> Self {
        Self {
            request_id: request_id.into(),
            http_status,
            exception_handler: None,
            exception_request: None,
        }
    }
}

tokio::task_local! {
    static RESPONSE_CONTEXT: ResponseContext;
}

/// 使用不可变的响应生成上下文执行请求 future。
pub async fn with_response_context<F: Future>(context: ResponseContext, future: F) -> F::Output {
    RESPONSE_CONTEXT.scope(context, future).await
}

/// Web middleware 在载入 Session 后更新同一请求的异常视图。
#[cfg(feature = "session")]
pub(crate) fn bind_exception_session(session: crate::session::Session) {
    let _ = RESPONSE_CONTEXT.try_with(|context| {
        if let Some(request) = &context.exception_request {
            request.bind_session(session);
        }
    });
}

/// 受管输入提取器只共享规范化输入，不共享原始正文或 Cookie。
#[cfg(feature = "session")]
pub(crate) fn bind_exception_input(input: serde_json::Value) {
    let _ = RESPONSE_CONTEXT.try_with(|context| {
        if let Some(request) = &context.exception_request {
            request.bind_input(input);
        }
    });
}

/// 可选输入投影不可表示正文时撤销先前的局部 Query 视图。
#[cfg(feature = "session")]
pub(crate) fn clear_exception_input() {
    let _ = RESPONSE_CONTEXT.try_with(|context| {
        if let Some(request) = &context.exception_request {
            request.clear_input();
        }
    });
}

fn response_context() -> ResponseContext {
    RESPONSE_CONTEXT
        .try_with(Clone::clone)
        .unwrap_or_else(|_| ResponseContext::new("-", HttpStatusMode::Semantic))
}

/// 返回当前请求使用的状态码策略。
pub fn current_http_status_mode() -> HttpStatusMode {
    response_context().http_status
}

/// 附加在成功分页响应中的类型化分页元数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PageMeta {
    /// 从 1 开始的页码。
    pub page: u64,
    /// 每页条目数。
    pub per_page: u64,
    /// 条目总数。
    pub total: u64,
    /// 总页数。
    pub total_pages: u64,
}

/// 成功与失败 API 响应共用的 JSON 封装。
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T: Serialize> {
    /// 操作是否成功。
    pub success: bool,
    /// 稳定且可供机器读取的错误码。
    pub code: String,
    /// 可安全展示给调用方的消息。
    pub message: String,
    /// 失败响应生成时的 RFC 3339 UTC 时间。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// 响应载荷；没有载荷时省略此字段。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// 可选的分页元数据。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<PageMeta>,
    /// 可选的字段级校验错误。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<ValidationErrors>,
    /// 失败响应中的请求关联 ID，与 `x-request-id` 响应头相同。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip)]
    request_id_header: String,
    #[serde(skip)]
    status: StatusCode,
    #[serde(skip)]
    mode: HttpStatusMode,
}

impl<T: Serialize> ApiResponse<T> {
    /// 包装成功响应载荷。
    pub fn success(data: T) -> Self {
        let context = response_context();
        Self {
            success: true,
            code: "ok".to_owned(),
            message: crate::i18n::translate_current(
                "openrust.response.ok",
                &crate::i18n::MessageArgs::new(),
            )
            .unwrap_or_else(|| "ok".to_owned()),
            timestamp: None,
            data: Some(data),
            meta: None,
            errors: None,
            request_id: None,
            request_id_header: context.request_id,
            status: StatusCode::OK,
            mode: context.http_status,
        }
    }

    /// 为成功响应添加分页元数据。
    #[must_use]
    pub fn with_meta(mut self, meta: PageMeta) -> Self {
        self.meta = Some(meta);
        self
    }

    /// 设置语义 HTTP 状态码，用于表示资源已创建。
    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    fn failure(
        code: &str,
        message: String,
        status: StatusCode,
        errors: Option<ValidationErrors>,
    ) -> Self {
        let context = response_context();
        Self {
            success: false,
            code: code.to_owned(),
            message,
            timestamp: Some(
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            ),
            data: None,
            meta: None,
            errors,
            request_id: Some(context.request_id.clone()),
            request_id_header: context.request_id,
            status,
            mode: context.http_status,
        }
    }
}

impl ApiResponse<()> {
    /// 创建不带业务载荷的成功响应。
    pub fn empty() -> Self {
        let mut response = Self::success(());
        response.data = None;
        response
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let request_id = HeaderValue::from_str(&self.request_id_header).ok();
        let mode = self.mode;
        let (semantic_status, body) = match serde_json::to_vec(&self) {
            Ok(body) => (self.status, body),
            Err(error) => {
                let _ = error;
                tracing::error!("response serialization failed");
                let mut fallback = Self::failure(
                    "internal_error",
                    "internal server error".to_owned(),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    None,
                );
                fallback.request_id = Some(self.request_id_header.clone());
                fallback
                    .request_id_header
                    .clone_from(&self.request_id_header);
                let body = serde_json::to_vec(&fallback).unwrap_or_else(|_| {
                    format!(
                        "{{\"success\":false,\"code\":\"internal_error\",\"message\":\"internal server error\",\"timestamp\":\"{}\",\"request_id\":{}}}",
                        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        serde_json::to_string(&self.request_id_header).unwrap_or_else(|_| "\"-\"".to_owned())
                    )
                    .into_bytes()
                });
                (StatusCode::INTERNAL_SERVER_ERROR, body)
            }
        };
        let status = if mode == HttpStatusMode::AlwaysOk {
            StatusCode::OK
        } else {
            semantic_status
        };
        let mut response = (status, body).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if let Some(request_id) = request_id {
            response.headers_mut().insert("x-request-id", request_id);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unscoped_error_uses_semantic_http_status_by_default() {
        let response = Error::NotFound("missing".to_owned()).into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn expected_http_overload_rejections_do_not_emit_error_logs() {
        let overloaded = Error::custom_with_status(
            StatusCode::SERVICE_UNAVAILABLE,
            "http_overloaded",
            "server request capacity is exhausted",
        );
        let internal = Error::internal(std::io::Error::other("database failure"));

        assert!(!should_log_server_error(&overloaded));
        assert!(should_log_server_error(&internal));
    }

    #[test]
    fn client_error_has_code_and_message() {
        let response = Error::NotFound("post 42 not found".into()).to_api_response();
        assert_eq!(response.code, "not_found");
        assert_eq!(response.message, "post 42 not found");
        assert!(!response.success);
    }

    #[test]
    fn internal_error_hides_source() {
        let response = Error::internal(std::io::Error::other("secret")).to_api_response();
        assert_eq!(response.message, "internal server error");
        assert!(
            !serde_json::to_string(&response)
                .unwrap_or_default()
                .contains("secret")
        );
    }

    #[test]
    fn validation_errors_are_listed_per_field() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "invalid");
        let response = Error::Validation(errors).to_api_response();
        assert_eq!(response.errors.as_ref().map(|e| e.iter().count()), Some(1));
    }

    #[test]
    fn custom_status_defaults_to_bad_request() {
        assert_eq!(
            Error::custom("quota_exceeded", "limit reached").status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            Error::custom_with_status(
                StatusCode::PAYMENT_REQUIRED,
                "quota_exceeded",
                "limit reached"
            )
            .status(),
            StatusCode::PAYMENT_REQUIRED
        );
    }

    #[test]
    fn page_meta_is_preserved_in_response() {
        let meta = PageMeta {
            page: 2,
            per_page: 20,
            total: 57,
            total_pages: 3,
        };
        let response = ApiResponse::success(vec![21_u64]).with_meta(meta);
        assert_eq!(response.meta, Some(meta));
        let json = serde_json::to_value(response).unwrap_or_default();
        assert_eq!(
            json["meta"],
            serde_json::json!({
                "page": 2,
                "per_page": 20,
                "total": 57,
                "total_pages": 3,
            })
        );
    }
}

#[cfg(test)]
#[path = "error/initialization_diagnostic_tests.rs"]
mod initialization_diagnostic_tests;
