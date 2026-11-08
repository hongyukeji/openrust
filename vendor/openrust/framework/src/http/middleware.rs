//! 默认 middleware stack。
//!
//! 从最外层到最内层依次为：规范化并设置 `x-request-id` → 将 ID 写入响应 → tracing span →
//! CORS → 响应上下文 → 状态映射 → 请求超时 → 压缩 → panic 捕获。通过 [`Router::layer`](crate::routing::Router::layer) 添加的应用 layer
//! 在此 stack 内运行。

use std::any::Any;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use crate::config::{ApiConfig, ConfigError, CorsConfig, ServerConfig};
use crate::core::CancelOnDrop;
use crate::core::Cancellation;
use crate::core::Error;
use crate::core::ExecutionContext;
use crate::core::ExecutionMetadata;
use crate::core::ResponseContext;
use crate::core::Result;
use crate::core::with_execution_context;
use crate::core::with_response_context;
use crate::foundation::ApplicationState;
use crate::foundation::ExceptionHandler;
use crate::foundation::ExceptionRequest;
use crate::support::BoxFuture;
use crate::translation::Translator;
use axum::Router;
use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

/// 签名 URL middleware 的实例选项；忽略集合按原始查询字段名匹配。
#[derive(Clone, Debug, Default)]
pub struct ValidateSignature {
    relative: bool,
    ignore: Vec<String>,
    global_except: Vec<String>,
}

impl ValidateSignature {
    /// 默认验证绝对 URL。
    pub fn new() -> Self {
        Self::default()
    }

    /// 验证相对 URL。
    #[must_use]
    pub fn relative(mut self) -> Self {
        self.relative = true;
        self
    }

    /// 设置当前 middleware 的局部忽略字段。
    #[must_use]
    pub fn ignoring(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.ignore = keys.into_iter().map(Into::into).collect();
        self
    }

    /// 设置本应用实例的全局忽略字段，由调用方在各签名层共享。
    #[must_use]
    pub fn except(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.global_except = keys.into_iter().map(Into::into).collect();
        self
    }
}

/// 默认绝对 URL 验签 middleware；非法签名返回 403，且不调用业务 handler。
pub async fn validate_signature(request: Request, next: axum::middleware::Next) -> Response {
    validate_signature_request(ValidateSignature::new(), request, next).await
}

/// 默认相对 URL 验签 middleware。
pub async fn validate_relative_signature(
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    validate_signature_request(ValidateSignature::new().relative(), request, next).await
}

/// 搭配 `from_fn_with_state` 使用局部忽略项与应用级 except。
pub async fn validate_signature_with_options(
    State(options): State<ValidateSignature>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    validate_signature_request(options, request, next).await
}

async fn validate_signature_request(
    options: ValidateSignature,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let (mut parts, body) = request.into_parts();
    let info = match crate::http::RequestInfo::from_request_parts(&mut parts, &()).await {
        Ok(info) => info,
        Err(error) => return error.into_response(),
    };
    let valid = info.has_valid_signature_ignoring(!options.relative, |key| {
        options
            .ignore
            .iter()
            .chain(options.global_except.iter())
            .any(|ignored| ignored == key)
    });
    match valid {
        Ok(true) => next.run(Request::from_parts(parts, body)).await,
        Ok(false) => crate::core::Error::Forbidden("Invalid signature".into()).into_response(),
        Err(error) => error.into_response(),
    }
}
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any as AnyOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

/// 容器解析的 HTTP 中间件，在正常响应完成后执行终止阶段。
///
/// `handle` 与 `terminate` 分别解析实例。普通绑定会创建新实例，共享绑定遵循
/// 容器的生命周期。终止阶段只借用头部及请求元数据，HTTP 正文沿流式传输交付。
pub trait TerminableMiddleware: Send + Sync + 'static {
    /// 处理请求。别名参数按声明顺序传入，可以调用 `next` 或直接返回响应。
    fn handle<'a>(
        &'a self,
        request: Request,
        next: axum::middleware::Next,
        parameters: &'a [String],
    ) -> BoxFuture<'a, Result<Response>>;

    /// 响应完成后处理请求和响应头。请求扩展含共享 `RequestAttributes`，已匹配时含 `Route`。
    fn terminate<'a>(
        &'a self,
        request: &'a http::Request<()>,
        response: &'a http::Response<()>,
    ) -> BoxFuture<'a, Result<()>>;
}

pub(crate) type TerminateMiddleware = for<'a> fn(
    &'a ApplicationState,
    &'a http::Request<()>,
    &'a http::Response<()>,
) -> BoxFuture<'a, Result<()>>;

pub(crate) async fn handle_terminable<M: TerminableMiddleware>(
    State(parameters): State<Arc<[String]>>,
    request: Request,
    next: axum::middleware::Next,
) -> Result<Response> {
    let scope = crate::foundation::http::http_entry_scope()
        .and_then(|scope| scope.upgrade())
        .ok_or_else(|| {
            Error::custom(
                "http_middleware_scope_unavailable",
                "终止中间件需要 HTTP 入口作用域",
            )
        })?;
    scope
        .resolve::<M>()
        .await?
        .handle(request, next, &parameters)
        .await
}

pub(crate) fn terminate_middleware<'a, M: TerminableMiddleware>(
    state: &'a ApplicationState,
    request: &'a http::Request<()>,
    response: &'a http::Response<()>,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        state
            .resolve::<M>()
            .await?
            .terminate(request, response)
            .await
    })
}

/// 使用默认 middleware stack 包装 `router`。
pub fn apply(
    router: Router,
    server: &ServerConfig,
    api: &ApiConfig,
    exceptions: Arc<ExceptionHandler>,
    debug: bool,
) -> Result<Router> {
    apply_with_middleware(router, server, api, exceptions, debug, true)
}

pub(crate) fn apply_with_middleware(
    router: Router,
    server: &ServerConfig,
    api: &ApiConfig,
    exceptions: Arc<ExceptionHandler>,
    debug: bool,
    middleware_enabled: bool,
) -> Result<Router> {
    let cors = if middleware_enabled && server.cors.enabled {
        Some(cors_layer(&server.cors)?)
    } else {
        None
    };
    let router = router.layer(CatchPanicLayer::custom(panic_response));
    let router = if server.compression.enabled {
        router.layer(CompressionLayer::new())
    } else {
        router
    };
    let router = router.layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(server.request_timeout_secs),
    ));
    let router = router.layer(axum::middleware::map_response(envelope_status));
    let router = router.layer(axum::middleware::from_fn_with_state(
        ResponseSettings {
            api: api.clone(),
            exceptions,
            debug,
            request_timeout: Duration::from_secs(server.request_timeout_secs),
            request_slots: Arc::new(tokio::sync::Semaphore::new(server.max_concurrent_requests)),
        },
        response_context,
    ));
    // CORS 也要包装刷新、admission 与绝对期限直接返回的响应。
    let router = if let Some(cors) = cors {
        router.layer(cors)
    } else {
        router
    };
    Ok(router
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        // 外层也覆盖 response_context 在调用 next 前返回的异常。
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(axum::middleware::from_fn(normalize_request_id)))
}

#[derive(Clone)]
struct ResponseSettings {
    api: ApiConfig,
    exceptions: Arc<ExceptionHandler>,
    debug: bool,
    request_timeout: Duration,
    request_slots: Arc<tokio::sync::Semaphore>,
}

async fn normalize_request_id(mut request: Request, next: axum::middleware::Next) -> Response {
    let invalid_request_id = request.headers().get("x-request-id").is_some_and(|value| {
        value.to_str().map_or(true, |request_id| {
            request_id.is_empty()
                || request_id.len() > 128
                || !request_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        })
    });
    if invalid_request_id {
        request.headers_mut().remove("x-request-id");
    }
    if request
        .headers()
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            crate::core::ExecutionMetadata::default()
                .with_traceparent(value.to_owned())
                .is_err()
        })
        || request
            .headers()
            .get("traceparent")
            .is_some_and(|value| value.to_str().is_err())
    {
        request.headers_mut().remove("traceparent");
    }
    next.run(request).await
}

#[allow(
    clippy::too_many_lines,
    reason = "the middleware owns request identity, limits, cancellation, tracing, and response wrapping"
)]
async fn response_context(
    State(settings): State<ResponseSettings>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    #[cfg(feature = "metrics")]
    let metric_method = crate::http::metrics::method_label(request.method());
    #[cfg(feature = "metrics")]
    let started = std::time::Instant::now();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    let exception_request = ExceptionRequest::new(
        request.method().clone(),
        request.uri().path(),
        crate::http::request::expects_json_headers(request.headers()),
        request_id.clone(),
        settings.debug,
    );
    #[cfg(feature = "view")]
    let exception_request = exception_request.with_views(
        request
            .extensions()
            .get::<crate::foundation::ApplicationState>()
            .and_then(crate::foundation::ApplicationState::get::<crate::view::Views>),
    );
    let locale_service = if let Some(state) = request
        .extensions()
        .get::<crate::foundation::ApplicationState>()
    {
        if state.container().contains::<Translator>() {
            match state.resolve::<Translator>().await {
                Ok(service) => Some(service),
                Err(error) => {
                    return with_response_context(
                        ResponseContext {
                            request_id,
                            http_status: settings.api.http_status,
                            exception_handler: Some(Arc::clone(&settings.exceptions)),
                            exception_request: Some(exception_request),
                        },
                        async move { error.into_response() },
                    )
                    .await;
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    let server_readiness = request
        .extensions()
        .get::<crate::foundation::ApplicationState>()
        .and_then(crate::foundation::ApplicationState::get::<crate::core::Readiness>);
    let accept_language = request
        .headers()
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let cancellation = server_readiness
        .as_ref()
        .map_or_else(Cancellation::new, |readiness| {
            readiness.request_cancellation()
        });
    let cancel_on_drop = CancelOnDrop::new(cancellation.clone());
    let deadline = tokio::time::Instant::now() + settings.request_timeout;
    let request_permit = settings.request_slots.clone().try_acquire_owned();
    // 仅获准请求捕获Source，且刷新与后续handler共享绝对期限和取消信号。
    let refresh_response = if request_permit.is_ok() {
        if let Some(i18n) = &locale_service {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => Some(true),
                () = tokio::time::sleep_until(deadline) => Some(false),
                _ = i18n.refresh_if_due() => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    // 刷新后只按原始头协商一次；metadata与task-local共享同一Locale。
    let locale = locale_service
        .as_ref()
        .map(|i18n| i18n.locale_for_request(accept_language.as_deref()));
    let mut metadata = ExecutionMetadata::default();
    if let Ok(updated) = metadata.clone().with_request_id(request_id.clone()) {
        metadata = updated;
    }
    if let Some(traceparent) = request
        .headers()
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        && let Ok(updated) = metadata.clone().with_traceparent(traceparent.to_owned())
    {
        metadata = updated;
    }
    if let Some(locale) = &locale {
        metadata = metadata.with_locale(locale.clone());
    }
    let execution = ExecutionContext::new(metadata, Some(deadline), cancellation.clone());

    #[cfg(feature = "metrics")]
    if request_permit.is_err() {
        metrics::counter!("openrust_http_overloaded_total").increment(1);
    }
    with_execution_context(execution, async move {
        let response = with_response_context(
            ResponseContext {
                request_id,
                http_status: settings.api.http_status,
                exception_handler: Some(settings.exceptions),
                exception_request: Some(exception_request),
            },
            async move {
                let _cancel_on_drop = cancel_on_drop;
                let request_permit = request_permit.ok();
                let response = if let Some(cancelled) = refresh_response {
                    if cancelled {
                        Error::custom_with_status(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "server_draining",
                            "server is shutting down",
                        )
                        .into_response()
                    } else {
                        timeout_response()
                    }
                } else if request_permit.is_some() {
                    tokio::select! {
                        biased;
                        response = next.run(request) => response,
                        () = tokio::time::sleep_until(deadline) => timeout_response(),
                        () = cancellation.cancelled() => Error::custom_with_status(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "server_draining",
                            "server is shutting down",
                        ).into_response(),
                    }
                } else {
                    Error::custom_with_status(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "http_overloaded",
                        "server request capacity is exhausted",
                    )
                    .into_response()
                };
                #[cfg(feature = "metrics")]
                let status_class = crate::http::metrics::status_class(response.status());
                response.map(|body| {
                    Body::new(RequestPermitBody {
                        inner: body,
                        permit: request_permit,
                        #[cfg(feature = "metrics")]
                        in_flight: Some(InFlightRequest::new(metric_method, status_class, started)),
                    })
                })
            },
        );
        match (locale_service, locale) {
            (Some(i18n), Some(locale)) => i18n.with_locale(locale, response).await,
            _ => response.await,
        }
    })
    .await
}

struct RequestPermitBody {
    inner: Body,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    #[cfg(feature = "metrics")]
    in_flight: Option<InFlightRequest>,
}

impl RequestPermitBody {
    fn release(&mut self) {
        self.permit.take();
        #[cfg(feature = "metrics")]
        self.in_flight.take();
    }
}

impl HttpBody for RequestPermitBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_frame(context);
        if matches!(&result, Poll::Ready(None | Some(Err(_)))) {
            this.release();
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

fn timeout_response() -> Response {
    Error::custom_with_status(
        StatusCode::REQUEST_TIMEOUT,
        "request_timeout",
        "request timed out",
    )
    .into_response()
}

async fn envelope_status(response: Response) -> Response {
    if response.status() == StatusCode::REQUEST_TIMEOUT
        && !response
            .headers()
            .contains_key(axum::http::header::CONTENT_TYPE)
    {
        timeout_response()
    } else if response.status() == StatusCode::PAYLOAD_TOO_LARGE
        && response.headers().get(axum::http::header::CONTENT_TYPE)
            != Some(&axum::http::HeaderValue::from_static("application/json"))
    {
        Error::custom_with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request body too large",
        )
        .into_response()
    } else if response.status() == StatusCode::INTERNAL_SERVER_ERROR
        && response.headers().get(axum::http::header::CONTENT_TYPE)
            != Some(&axum::http::HeaderValue::from_static("application/json"))
    {
        Error::internal(UnwrappedInternalResponse).into_response()
    } else {
        response
    }
}

#[derive(Debug)]
struct UnwrappedInternalResponse;

impl std::fmt::Display for UnwrappedInternalResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an HTTP middleware returned an unwrapped internal error response")
    }
}

impl std::error::Error for UnwrappedInternalResponse {}

fn invalid(key: &str, detail: &str) -> Error {
    ConfigError::Invalid(format!("{key}: {detail}")).into()
}

fn parse_list<T: std::str::FromStr>(values: &[String], key: &str) -> Result<Vec<T>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.parse().map_err(|_| {
                invalid(
                    &format!("server.cors.{key}[{index}]"),
                    &format!("invalid value {value:?}"),
                )
            })
        })
        .collect()
}

fn validate_wildcard(values: &[String], key: &str, allow_credentials: bool) -> Result<()> {
    if let Some(index) = values.iter().position(|value| value == "*") {
        let item_key = format!("server.cors.{key}[{index}]");
        if allow_credentials {
            return Err(invalid(
                &item_key,
                "cannot be \"*\" when server.cors.allow_credentials = true",
            ));
        }
        if values.len() != 1 {
            return Err(invalid(
                &item_key,
                "\"*\" cannot be mixed with concrete values",
            ));
        }
    }
    Ok(())
}

fn validate_origin(origin: &str, index: usize) -> Result<HeaderValue> {
    let key = format!("server.cors.allowed_origins[{index}]");
    let valid = origin.parse::<Uri>().is_ok_and(|uri| {
        matches!(uri.scheme_str(), Some("http" | "https"))
            && uri.authority().is_some_and(|authority| {
                let authority_text = authority.as_str();
                let has_port = if authority_text.starts_with('[') {
                    authority_text
                        .split_once(']')
                        .is_some_and(|(_, rest)| rest.starts_with(':'))
                } else {
                    authority_text.contains(':')
                };
                !authority.host().is_empty()
                    && !authority_text.contains('@')
                    && (!has_port || authority.port_u16().is_some())
                    && origin == format!("{}://{authority}", uri.scheme_str().unwrap_or_default())
            })
    });
    if !valid {
        return Err(invalid(&key, &format!("invalid origin {origin:?}")));
    }
    origin
        .parse()
        .map_err(|_| invalid(&key, &format!("invalid origin {origin:?}")))
}

fn cors_layer(config: &CorsConfig) -> Result<CorsLayer> {
    if config.allowed_origins.is_empty() {
        return Err(invalid(
            "server.cors.allowed_origins",
            "must not be empty when enabled",
        ));
    }
    let wildcard = config.allowed_origins.iter().any(|origin| origin == "*");
    if wildcard && config.allowed_origins.len() != 1 {
        return Err(invalid(
            "server.cors.allowed_origins",
            "* cannot be mixed with origins",
        ));
    }
    if wildcard && config.allow_credentials {
        return Err(invalid(
            "server.cors.allow_credentials",
            "cannot be true with server.cors.allowed_origins = [\"*\"]",
        ));
    }
    let mut layer = CorsLayer::new();
    if wildcard {
        layer = layer.allow_origin(AnyOrigin);
    } else {
        layer = layer.allow_origin(
            config
                .allowed_origins
                .iter()
                .enumerate()
                .map(|(index, origin)| validate_origin(origin, index))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    validate_wildcard(
        &config.allowed_methods,
        "allowed_methods",
        config.allow_credentials,
    )?;
    validate_wildcard(
        &config.allowed_headers,
        "allowed_headers",
        config.allow_credentials,
    )?;
    validate_wildcard(
        &config.expose_headers,
        "expose_headers",
        config.allow_credentials,
    )?;
    let methods = parse_list::<Method>(&config.allowed_methods, "allowed_methods")?;
    let headers = parse_list::<HeaderName>(&config.allowed_headers, "allowed_headers")?;
    let exposed = parse_list::<HeaderName>(&config.expose_headers, "expose_headers")?;
    layer = layer
        .allow_methods(methods)
        .allow_headers(headers)
        .expose_headers(exposed)
        .allow_credentials(config.allow_credentials)
        .max_age(Duration::from_secs(config.max_age_secs));
    Ok(layer)
}

fn request_span(request: &Request) -> tracing::Span {
    let method = match request.method().as_str() {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD" => {
            request.method().as_str()
        }
        _ => "OTHER",
    };
    let span = tracing::info_span!("request", method,);
    #[cfg(feature = "otel")]
    crate::core::otel::set_remote_parent(
        &span,
        request
            .headers()
            .get("traceparent")
            .and_then(|value| value.to_str().ok()),
    );
    span
}

#[cfg(feature = "metrics")]
struct InFlightRequest {
    method: &'static str,
    status_class: &'static str,
    started: std::time::Instant,
}

#[cfg(feature = "metrics")]
impl InFlightRequest {
    fn new(method: &'static str, status_class: &'static str, started: std::time::Instant) -> Self {
        metrics::gauge!("openrust_http_requests_in_flight").increment(1.0);
        Self {
            method,
            status_class,
            started,
        }
    }
}

#[cfg(feature = "metrics")]
impl Drop for InFlightRequest {
    fn drop(&mut self) {
        metrics::gauge!("openrust_http_requests_in_flight").decrement(1.0);
        metrics::counter!("openrust_http_requests_total", "method" => self.method, "status_class" => self.status_class).increment(1);
        metrics::histogram!("openrust_http_request_duration_seconds", "method" => self.method, "status_class" => self.status_class).record(self.started.elapsed().as_secs_f64());
    }
}

#[derive(Debug, thiserror::Error)]
#[error("handler panicked: {0}")]
struct HandlerPanicked(String);

// `CatchPanicLayer::custom` 要求按值传入 payload。
#[allow(clippy::needless_pass_by_value)]
fn panic_response(payload: Box<dyn Any + Send + 'static>) -> Response {
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "non-string panic payload".to_owned());
    Error::internal(HandlerPanicked(message)).into_response()
}

#[cfg(test)]
mod tests {
    use super::apply;
    use super::request_span;
    use crate::config::ApiConfig;
    use crate::config::HttpStatusMode;
    use crate::config::ServerConfig;
    use crate::core::ExecutionContext;
    use crate::foundation::ExceptionHandler;
    use axum::{
        Router, body::Body, extract::Request, http::StatusCode, response::Response, routing::get,
    };
    use bytes::Bytes;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::Notify;
    use tower::ServiceExt;

    #[test]
    fn request_span_does_not_capture_identifiers_or_full_uri() -> crate::core::Result<()> {
        let request = Request::builder()
            .uri("/users/private-id?token=secret")
            .header("x-request-id", "request-secret")
            .header(
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            )
            .body(Body::empty())
            .map_err(crate::core::Error::internal)?;
        let span = request_span(&request);
        let names = span.metadata().map(|metadata| {
            metadata
                .fields()
                .iter()
                .map(|field| field.name())
                .collect::<Vec<_>>()
        });
        assert_eq!(names, Some(vec!["method"]));
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_request_limit_rejects_without_waiting_and_recovers()
    -> crate::core::Result<()> {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let handler_started = Arc::clone(&started);
        let handler_release = Arc::clone(&release);
        let router = Router::new().route(
            "/wait",
            get(move || {
                let started = Arc::clone(&handler_started);
                let release = Arc::clone(&handler_release);
                async move {
                    started.notify_one();
                    release.notified().await;
                    "done"
                }
            }),
        );
        let server = ServerConfig {
            max_concurrent_requests: 1,
            ..ServerConfig::default()
        };
        let api = ApiConfig {
            http_status: HttpStatusMode::Semantic,
            ..ApiConfig::default()
        };
        let app = apply(
            router,
            &server,
            &api,
            Arc::new(ExceptionHandler::default()),
            false,
        )?;
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            first_app
                .oneshot(
                    Request::get("/wait")
                        .body(Body::empty())
                        .map_err(crate::core::Error::internal)?,
                )
                .await
                .map_err(crate::core::Error::internal)
        });
        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .map_err(crate::core::Error::internal)?;

        let rejected = app
            .clone()
            .oneshot(
                Request::get("/wait")
                    .body(Body::empty())
                    .map_err(crate::core::Error::internal)?,
            )
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
        release.notify_one();
        let completed = tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .map_err(crate::core::Error::internal)?
            .map_err(crate::core::Error::internal)??;
        assert_eq!(completed.status(), StatusCode::OK);
        drop(completed);

        let recovered_app = app.clone();
        let recovered = tokio::spawn(async move {
            recovered_app
                .oneshot(
                    Request::get("/wait")
                        .body(Body::empty())
                        .map_err(crate::core::Error::internal)?,
                )
                .await
                .map_err(crate::core::Error::internal)
        });
        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .map_err(crate::core::Error::internal)?;
        release.notify_one();
        let recovered = tokio::time::timeout(Duration::from_secs(1), recovered)
            .await
            .map_err(crate::core::Error::internal)?
            .map_err(crate::core::Error::internal)??;
        assert_eq!(recovered.status(), StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_request_limit_covers_unconsumed_response_bodies() -> crate::core::Result<()>
    {
        let router = Router::new().route(
            "/stream",
            get(|| async {
                Response::new(Body::from_stream(futures_util::stream::pending::<
                    std::result::Result<Bytes, std::convert::Infallible>,
                >()))
            }),
        );
        let server = ServerConfig {
            max_concurrent_requests: 1,
            ..ServerConfig::default()
        };
        let api = ApiConfig {
            http_status: HttpStatusMode::Semantic,
            ..ApiConfig::default()
        };
        let app = apply(
            router,
            &server,
            &api,
            Arc::new(ExceptionHandler::default()),
            false,
        )?;
        let active = app
            .clone()
            .oneshot(
                Request::get("/stream")
                    .body(Body::empty())
                    .map_err(crate::core::Error::internal)?,
            )
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(active.status(), StatusCode::OK);

        let rejected = app
            .clone()
            .oneshot(
                Request::get("/stream")
                    .body(Body::empty())
                    .map_err(crate::core::Error::internal)?,
            )
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
        drop(active);

        let recovered = app
            .oneshot(
                Request::get("/stream")
                    .body(Body::empty())
                    .map_err(crate::core::Error::internal)?,
            )
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(recovered.status(), StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn request_context_accepts_only_valid_traceparent()
    -> Result<(), Box<dyn std::error::Error>> {
        let router = Router::new().route(
            "/",
            get(|| async {
                ExecutionContext::current()
                    .and_then(|context| context.metadata().traceparent().map(str::to_owned))
                    .unwrap_or_default()
            }),
        );
        let app = apply(
            router,
            &ServerConfig::default(),
            &ApiConfig::default(),
            Arc::new(ExceptionHandler::default()),
            false,
        )?;
        let valid = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("traceparent", valid)
                    .body(axum::body::Body::empty())?,
            )
            .await?;
        let body = axum::body::to_bytes(response.into_body(), 128).await?;
        assert_eq!(body.as_ref(), valid.as_bytes());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("traceparent", "not-a-traceparent")
                    .body(axum::body::Body::empty())?,
            )
            .await?;
        let body = axum::body::to_bytes(response.into_body(), 128).await?;
        assert!(body.is_empty());
        Ok(())
    }
}

#[cfg(test)]
#[path = "middleware_source_tests.rs"]
mod source_tests;
