//! 与应用同生命周期的异常报告和 HTTP 响应定制。

use std::error::Error as StdError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum_core::response::{IntoResponse, Response};
use http::{HeaderValue, Method, header};

use super::{Application, BootContext};
use crate::core::{Error, Result};

type Matcher = Arc<dyn Fn(&Error) -> bool + Send + Sync>;
type Mapper = Arc<dyn Fn(&Error) -> Option<Error> + Send + Sync>;
type Reporter = Arc<dyn Fn(&Error) -> Option<ReportDecision> + Send + Sync>;
type Renderer = Arc<dyn Fn(&Error, &ExceptionRequest) -> Option<Response> + Send + Sync>;
type Finalizer = Arc<dyn Fn(Response, &Error, &ExceptionRequest) -> Response + Send + Sync>;
type JsonDecision = Arc<dyn Fn(&ExceptionRequest, &Error) -> bool + Send + Sync>;
type ContextCallback = Arc<dyn Fn(&Error) -> Vec<(String, String)> + Send + Sync>;
type LevelCallback = Arc<dyn Fn(&Error) -> Option<tracing::Level> + Send + Sync>;
type RedirectTarget = Arc<dyn Fn(&ExceptionRequest) -> Option<String> + Send + Sync>;
type ThrottleCallback = Arc<dyn Fn(&Error) -> Option<ReportThrottle> + Send + Sync>;

/// 报告限频策略；限制键应按异常类别和业务维度稳定生成。
#[derive(Debug, Clone)]
pub enum ReportThrottle {
    /// 本次异常不受限频约束。
    Unlimited,
    /// 以 `numerator / denominator` 的概率报告。
    Lottery {
        /// 命中数量。
        numerator: u32,
        /// 总抽样数量，必须大于零。
        denominator: u32,
    },
    /// 当前应用实例内的固定窗口报告配额。
    Limit {
        /// 限制键；不应包含原始凭据。
        key: String,
        /// 窗口内最多报告次数。
        max_attempts: u32,
        /// 窗口时长。
        window: Duration,
    },
}

/// 报告回调是否阻止后续回调和默认日志；渲染仍会继续。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportDecision {
    /// 继续执行后续报告回调与默认日志。
    Continue,
    /// 停止报告链，但仍渲染异常。
    Stop,
}

/// 与请求绑定的安全元数据；不持有正文、Cookie 或完整 URI 查询串。
#[derive(Clone)]
pub struct ExceptionRequest {
    method: Method,
    path: String,
    expects_json: bool,
    request_id: String,
    debug: bool,
    #[cfg(feature = "view")]
    views: Option<Arc<crate::view::Views>>,
    #[cfg(feature = "session")]
    session: Arc<std::sync::Mutex<Option<crate::session::Session>>>,
    #[cfg(feature = "session")]
    input: Arc<std::sync::Mutex<Option<serde_json::Value>>>,
}

impl std::fmt::Debug for ExceptionRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExceptionRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("expects_json", &self.expects_json)
            .field("debug", &self.debug)
            .finish_non_exhaustive()
    }
}

impl ExceptionRequest {
    /// 从已校验的请求元数据建立异常视图，不复制任意请求头。
    pub fn new(
        method: Method,
        path: impl Into<String>,
        expects_json: bool,
        request_id: impl Into<String>,
        debug: bool,
    ) -> Self {
        Self {
            method,
            path: path.into(),
            expects_json,
            request_id: request_id.into(),
            debug,
            #[cfg(feature = "view")]
            views: None,
            #[cfg(feature = "session")]
            session: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "session")]
            input: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// 安装当前应用已登记的错误视图渲染器。
    #[cfg(feature = "view")]
    #[must_use]
    pub fn with_views(mut self, views: Option<Arc<crate::view::Views>>) -> Self {
        self.views = views;
        self
    }

    /// HTTP 方法。
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// 不包含 query 的请求路径。
    pub fn path(&self) -> &str {
        &self.path
    }

    /// 默认内容协商是否期望 JSON。
    pub fn expects_json(&self) -> bool {
        self.expects_json
    }

    /// 本请求的关联 ID。
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// 服务端配置的调试状态；客户端头不能启用。
    pub fn debug(&self) -> bool {
        self.debug
    }

    /// 将 Web 组已加载的 Session 绑定到同一请求异常视图。
    #[cfg(feature = "session")]
    pub(crate) fn bind_session(&self, session: crate::session::Session) {
        let mut slot = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(session);
    }

    #[cfg(feature = "session")]
    fn session(&self) -> Option<crate::session::Session> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// 将受管 Request 提取器规范化后的输入绑定给校验失败重定向。
    #[cfg(feature = "session")]
    pub(crate) fn bind_input(&self, input: serde_json::Value) {
        let mut slot = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(input);
    }

    #[cfg(feature = "session")]
    pub(crate) fn clear_input(&self) {
        let mut slot = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = None;
    }

    #[cfg(feature = "session")]
    fn input(&self) -> Option<serde_json::Value> {
        self.input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

type ThrottleState =
    Arc<std::sync::Mutex<std::collections::HashMap<String, (Instant, u32, Duration)>>>;

/// 按登记顺序应用异常处理策略。
#[derive(Clone, Default)]
pub struct ExceptionHandler {
    i18n: Option<crate::translation::Translator>,
    mappers: Vec<Mapper>,
    ignored: Vec<(std::any::TypeId, Matcher)>,
    unignored: Vec<Matcher>,
    ignore_when: Vec<Matcher>,
    no_retry: Vec<Matcher>,
    no_retry_when: Vec<Matcher>,
    reporters: Vec<Reporter>,
    renderers: Vec<Renderer>,
    response_finalizer: Option<Finalizer>,
    #[cfg(feature = "auth-fortify")]
    no_store_error_paths: Vec<crate::config::ApiConfig>,
    json_decision: Option<JsonDecision>,
    api_envelope: Option<JsonDecision>,
    unauthenticated_redirect: Option<RedirectTarget>,
    context_callbacks: Vec<ContextCallback>,
    levels: Vec<LevelCallback>,
    throttles: Vec<ThrottleCallback>,
    throttle_state: ThrottleState,
    without_duplicates: bool,
    reported: Arc<std::sync::Mutex<Vec<std::sync::Weak<Error>>>>,
    #[cfg(feature = "session")]
    dont_flash: Vec<String>,
}

impl ExceptionHandler {
    pub(crate) fn set_i18n(&mut self, service: crate::translation::Translator) {
        self.i18n = Some(service);
    }

    pub(crate) fn i18n_service(&self) -> Option<&crate::translation::Translator> {
        self.i18n.as_ref()
    }

    /// 除内置密码字段外，声明校验失败重定向时不得 flash 的输入字段。
    #[cfg(feature = "session")]
    pub fn dont_flash<I, S>(&mut self, attributes: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for attribute in attributes.into_iter().map(Into::into) {
            if !self.dont_flash.contains(&attribute) {
                self.dont_flash.push(attribute);
            }
        }
        self
    }
    /// 映射指定具体异常类型；首次返回映射结果者胜出。
    pub fn map<E, F>(&mut self, mapper: F) -> &mut Self
    where
        E: StdError + Send + Sync + 'static,
        F: Fn(&E) -> Error + Send + Sync + 'static,
    {
        self.mappers
            .push(Arc::new(move |error| typed_source::<E>(error).map(&mapper)));
        self
    }

    /// 登记按具体异常类型运行的报告回调。
    pub fn report<E, F>(&mut self, callback: F) -> &mut Self
    where
        E: StdError + Send + Sync + 'static,
        F: Fn(&E) -> ReportDecision + Send + Sync + 'static,
    {
        self.reporters.push(Arc::new(move |error| {
            typed_source::<E>(error).map(&callback)
        }));
        self
    }

    /// 指定异常类型不进入报告链；仍正常渲染响应。
    pub fn dont_report<E: StdError + Send + Sync + 'static>(&mut self) -> &mut Self {
        self.ignored.push((
            std::any::TypeId::of::<E>(),
            Arc::new(|error| typed_source::<E>(error).is_some()),
        ));
        self
    }

    /// 删除指定类型的显式忽略登记，并覆盖内置 HTTP 异常忽略规则。
    pub fn stop_ignoring<E: StdError + Send + Sync + 'static>(&mut self) -> &mut Self {
        self.ignored
            .retain(|(kind, _)| *kind != std::any::TypeId::of::<E>());
        self.unignored
            .push(Arc::new(|error| typed_source::<E>(error).is_some()));
        self
    }

    /// 动态决定是否跳过报告。
    pub fn dont_report_when<F>(&mut self, predicate: F) -> &mut Self
    where
        F: Fn(&Error) -> bool + Send + Sync + 'static,
    {
        self.ignore_when.push(Arc::new(predicate));
        self
    }

    /// 阻止指定具体异常类型进入应用的重试队列，与报告开关独立。
    pub fn dont_retry<E: StdError + Send + Sync + 'static>(&mut self) -> &mut Self {
        self.no_retry
            .push(Arc::new(|error| typed_source::<E>(error).is_some()));
        self
    }

    /// 动态决定一个异常是否应停止重试。
    pub fn dont_retry_when<F>(&mut self, predicate: F) -> &mut Self
    where
        F: Fn(&Error) -> bool + Send + Sync + 'static,
    {
        self.no_retry_when.push(Arc::new(predicate));
        self
    }

    /// 供队列或任务执行器查询；HTTP 渲染不会自行安排重试。
    pub fn should_stop_retries(&self, error: &Error) -> bool {
        self.no_retry.iter().any(|matches| matches(error))
            || self.no_retry_when.iter().any(|matches| matches(error))
    }

    /// 判断异常是否进入报告回调与默认日志，不影响 HTTP 响应。
    pub fn should_report(&self, error: &Error) -> bool {
        (!builtin_ignored(error) || self.unignored.iter().any(|matches| matches(error)))
            && !self.ignored.iter().any(|(_, matches)| matches(error))
            && !self.ignore_when.iter().any(|predicate| predicate(error))
            && self
                .throttles
                .iter()
                .find_map(|throttle| throttle(error))
                .is_none_or(|policy| self.allow_report(policy))
    }

    /// 登记逐具体异常类型的报告限频；第一个返回策略的回调生效。
    pub fn throttle<E, F>(&mut self, callback: F) -> &mut Self
    where
        E: StdError + Send + Sync + 'static,
        F: Fn(&E) -> Option<ReportThrottle> + Send + Sync + 'static,
    {
        self.throttles.push(Arc::new(move |error| {
            typed_source::<E>(error).and_then(&callback)
        }));
        self
    }

    fn allow_report(&self, policy: ReportThrottle) -> bool {
        match policy {
            ReportThrottle::Unlimited => true,
            ReportThrottle::Lottery {
                numerator,
                denominator,
            } => {
                if denominator == 0 || numerator >= denominator {
                    return numerator > 0;
                }
                let mut bytes = [0_u8; 8];
                getrandom::fill(&mut bytes).map_or(true, |()| {
                    u64::from_ne_bytes(bytes) % u64::from(denominator) < u64::from(numerator)
                })
            }
            ReportThrottle::Limit {
                key,
                max_attempts,
                window,
            } => {
                if max_attempts == 0 || window.is_zero() {
                    return false;
                }
                let now = Instant::now();
                let mut state = self
                    .throttle_state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.len() >= 4096 {
                    state.retain(|_, (started, _, span)| now.duration_since(*started) < *span);
                    if state.len() >= 4096 && !state.contains_key(&key) {
                        return true;
                    }
                }
                let entry = state.entry(key).or_insert((now, 0, window));
                if now.duration_since(entry.0) >= entry.2 || entry.2 != window {
                    *entry = (now, 0, window);
                }
                if entry.1 >= max_attempts {
                    false
                } else {
                    entry.1 += 1;
                    true
                }
            }
        }
    }

    /// 指定具体异常类型的默认日志级别。
    pub fn level<E: StdError + Send + Sync + 'static>(
        &mut self,
        level: tracing::Level,
    ) -> &mut Self {
        self.levels.push(Arc::new(move |error| {
            typed_source::<E>(error).map(|_| level)
        }));
        self
    }

    /// 增加显式日志上下文。应用对回调输出的敏感信息负责。
    pub fn context<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(&Error) -> Vec<(String, String)> + Send + Sync + 'static,
    {
        self.context_callbacks.push(Arc::new(callback));
        self
    }

    /// 登记按异常具体类型和请求渲染的回调；返回 `None` 则继续下一个。
    pub fn render<E, F>(&mut self, callback: F) -> &mut Self
    where
        E: StdError + Send + Sync + 'static,
        F: Fn(&E, &ExceptionRequest) -> Option<Response> + Send + Sync + 'static,
    {
        self.renderers.push(Arc::new(move |error, request| {
            typed_source::<E>(error).and_then(|source| callback(source, request))
        }));
        self
    }

    /// 覆盖默认 `expects_json` 判定。
    pub fn should_render_json_when<F>(&mut self, predicate: F) -> &mut Self
    where
        F: Fn(&ExceptionRequest, &Error) -> bool + Send + Sync + 'static,
    {
        self.json_decision = Some(Arc::new(predicate));
        self
    }

    /// 对选定请求使用 OpenRust 原有统一 API envelope。
    pub fn api_envelope_when<F>(&mut self, predicate: F) -> &mut Self
    where
        F: Fn(&ExceptionRequest, &Error) -> bool + Send + Sync + 'static,
    {
        self.api_envelope = Some(Arc::new(predicate));
        self
    }

    /// 为模块追加统一响应范围，同时保留应用已选择的范围。
    ///
    /// `api_envelope_when` 仍显式替换策略；此方法按逻辑或组合现有策略。
    pub fn or_api_envelope_when<F>(&mut self, predicate: F) -> &mut Self
    where
        F: Fn(&ExceptionRequest, &Error) -> bool + Send + Sync + 'static,
    {
        let previous = self.api_envelope.take();
        self.api_envelope = Some(Arc::new(move |request, error| {
            predicate(request, error)
                || previous
                    .as_ref()
                    .is_some_and(|previous| previous(request, error))
        }));
        self
    }

    /// 为浏览器未认证请求指定登录地址；JSON 请求继续返回 401。
    pub fn redirect_unauthenticated_to<F>(&mut self, target: F) -> &mut Self
    where
        F: Fn(&ExceptionRequest) -> Option<String> + Send + Sync + 'static,
    {
        self.unauthenticated_redirect = Some(Arc::new(target));
        self
    }

    /// 定制最终已渲染响应；回调可原样保留 status、头、扩展与流正文。
    pub fn respond<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(Response, &Error, &ExceptionRequest) -> Response + Send + Sync + 'static,
    {
        self.response_finalizer = Some(Arc::new(callback));
        self
    }

    /// 显式凭据API的异常响应不能因全局超时或异常重建而重新允许缓存。
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn no_store_errors_under(&mut self, prefix: String) {
        let policy = crate::config::ApiConfig {
            prefix,
            ..crate::config::ApiConfig::default()
        };
        if !self.no_store_error_paths.contains(&policy) {
            self.no_store_error_paths.push(policy);
        }
    }

    /// CLI、任务或 HTTP 均可报告，只有 HTTP 请求才进入渲染链。
    pub fn report_error(&self, error: &Error) {
        let mapped = self.mappers.iter().find_map(|mapper| mapper(error));
        self.report_mapped(mapped.as_ref().unwrap_or(error));
    }

    /// 同一个共享异常实例仅报告一次；以 `Arc` 身份代替 PHP 对象身份。
    pub fn dont_report_duplicates(&mut self) -> &mut Self {
        self.without_duplicates = true;
        self
    }

    /// 在启用去重时以共享异常实例身份过滤重复报告。
    pub fn report_shared(&self, error: &Arc<Error>) {
        if self.without_duplicates {
            let mut reported = self
                .reported
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            reported.retain(|known| known.strong_count() > 0);
            if reported
                .iter()
                .any(|known| known.ptr_eq(&Arc::downgrade(error)))
            {
                return;
            }
            reported.push(Arc::downgrade(error));
        }
        self.report_error(error);
    }

    fn report_mapped(&self, error: &Error) {
        if !self.should_report(error) {
            return;
        }
        for reporter in &self.reporters {
            if reporter(error) == Some(ReportDecision::Stop) {
                return;
            }
        }
        let level = self
            .levels
            .iter()
            .rev()
            .find_map(|mapping| mapping(error))
            .unwrap_or(tracing::Level::ERROR);
        let context = self
            .context_callbacks
            .iter()
            .flat_map(|callback| callback(error))
            .collect::<Vec<_>>();
        let code = error.code();
        let status = error.status().as_u16();
        match level {
            tracing::Level::ERROR => tracing::error!(code, status, ?context, "exception reported"),
            tracing::Level::WARN => tracing::warn!(code, status, ?context, "exception reported"),
            tracing::Level::INFO => tracing::info!(code, status, ?context, "exception reported"),
            tracing::Level::DEBUG => tracing::debug!(code, status, ?context, "exception reported"),
            tracing::Level::TRACE => tracing::trace!(code, status, ?context, "exception reported"),
        }
    }

    /// 报告并渲染请求异常；typed 回调在默认协商前运行。
    pub fn render_for_request(&self, error: Error, request: &ExceptionRequest) -> Response {
        let error = self
            .mappers
            .iter()
            .find_map(|mapper| mapper(&error))
            .unwrap_or(error);
        self.report_mapped(&error);
        let response = self
            .renderers
            .iter()
            .find_map(|renderer| renderer(&error, request))
            .unwrap_or_else(|| self.default_response(&error, request));
        let mut response = if let Some(finalize) = &self.response_finalizer {
            finalize(response, &error, request)
        } else {
            response
        };
        #[cfg(feature = "auth-fortify")]
        if self
            .no_store_error_paths
            .iter()
            .any(|policy| policy.matches_path(request.path()))
        {
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
                .headers_mut()
                .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        }
        if request.method() == Method::HEAD {
            // 不读取流；保留最终状态、头和扩展，直接释放原正文。
            *response.body_mut() = axum::body::Body::empty();
        }
        response
            .extensions_mut()
            .insert(crate::http::middleware::RenderedErrorResponse);
        response
    }

    fn default_response(&self, error: &Error, request: &ExceptionRequest) -> Response {
        let envelope = self
            .api_envelope
            .as_ref()
            .is_some_and(|predicate| predicate(request, error));
        let json = self
            .json_decision
            .as_ref()
            .map_or(request.expects_json(), |predicate| {
                predicate(request, error)
            });
        let mut response = if envelope {
            error
                .to_api_response_with_i18n(self.i18n.as_ref())
                .into_response()
        } else if json {
            let message = error.user_message(self.i18n.as_ref());
            let mut payload = serde_json::json!({"message": message});
            if let Error::Validation(errors)
            | Error::ValidationDetail { errors, .. }
            | Error::ValidationInBag { errors, .. } = error
                && let Ok(value) = serde_json::to_value(errors)
            {
                payload["errors"] = value;
            }
            if request.debug() && error.status().is_server_error() {
                payload["sources"] = serde_json::json!(debug_sources(error));
            }
            (error.status(), axum::Json(payload)).into_response()
        } else {
            let status = error.status();
            if matches!(error, Error::Unauthorized)
                && let Some(target) = self
                    .unauthenticated_redirect
                    .as_ref()
                    .and_then(|resolve| resolve(request))
                && let Ok(location) = HeaderValue::from_str(&target)
            {
                let mut response = http::StatusCode::FOUND.into_response();
                response.headers_mut().insert(header::LOCATION, location);
                return response;
            }
            #[cfg(feature = "session")]
            if let Some(response) = self.validation_redirect(error, request) {
                return response;
            }
            let fallback = || {
                let message = escape_html(&error.user_message(self.i18n.as_ref()));
                let title = escape_html(
                    &crate::translation::translate_for_rendering(
                        self.i18n.as_ref(),
                        "openrust.error.title",
                        &crate::translation::MessageArgs::new(),
                    )
                    .unwrap_or_default(),
                );
                let locale = crate::translation::rendering_locale(self.i18n.as_ref());
                let status_code = status.as_u16();
                let debug = if request.debug() && status.is_server_error() {
                    debug_sources(error)
                        .into_iter()
                        .fold(String::new(), |mut html, source| {
                            use std::fmt::Write as _;
                            let _ = write!(html, "<li>{}</li>", escape_html(&source));
                            html
                        })
                } else {
                    String::new()
                };
                let body = format!(
                    "<!doctype html><html lang=\"{locale}\"><meta charset=\"utf-8\"><title>{status_code} {title}</title><body><h1>{status_code} {title}</h1><p>{message}</p><ol>{debug}</ol></body></html>"
                );
                let mut response = (status, body).into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                );
                response
            };
            #[cfg(feature = "view")]
            {
                self.error_view(error, request).unwrap_or_else(fallback)
            }
            #[cfg(not(feature = "view"))]
            {
                fallback()
            }
        };
        if let Error::RateLimitedWithRetryAfter(seconds) = error
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static("Accept"));
        response
    }

    #[cfg(feature = "session")]
    fn validation_redirect(&self, error: &Error, request: &ExceptionRequest) -> Option<Response> {
        let (Error::Validation(errors)
        | Error::ValidationDetail { errors, .. }
        | Error::ValidationInBag { errors, .. }) = error
        else {
            return None;
        };
        let session = request.session()?;
        let mut bag = crate::support::MessageBag::new();
        for (field, messages) in errors.iter() {
            for message in messages {
                if let Err(flash_error) = bag.add(field.to_owned(), message.clone()) {
                    self.report_error(&flash_error);
                    return None;
                }
            }
        }
        let override_bag = request.input().and_then(|input| {
            input
                .get("_error_bag")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
        if let Err(flash_error) = session.flash_errors(
            override_bag
                .as_deref()
                .unwrap_or_else(|| error.validation_error_bag().unwrap_or("default")),
            &bag,
        ) {
            self.report_error(&flash_error);
            return None;
        }
        if let Some(mut input) = request.input() {
            for sensitive in ["current_password", "password", "password_confirmation"]
                .into_iter()
                .chain(self.dont_flash.iter().map(String::as_str))
            {
                if let Err(flash_error) = crate::support::data::forget(&mut input, sensitive) {
                    self.report_error(&flash_error);
                    return None;
                }
            }
            if let Err(flash_error) = session.flash_input_typed(input) {
                self.report_error(&flash_error);
                return None;
            }
        }
        let target = session
            .previous_url()
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "/".to_owned());
        let location =
            HeaderValue::from_str(&target).unwrap_or_else(|_| HeaderValue::from_static("/"));
        let mut response = http::StatusCode::FOUND.into_response();
        response.headers_mut().insert(header::LOCATION, location);
        Some(response)
    }

    #[cfg(feature = "view")]
    fn error_view(&self, error: &Error, request: &ExceptionRequest) -> Option<Response> {
        let views = request.views.as_ref()?;
        let status = error.status();
        let context = serde_json::json!({"message": error.user_message(self.i18n.as_ref()), "locale": crate::translation::rendering_locale(self.i18n.as_ref()).as_str(), "status": error.status().as_u16()});
        for name in [
            format!("errors/{}.html", status.as_u16()),
            format!("errors/{}xx.html", status.as_u16() / 100),
        ] {
            match views.render(&name, &context) {
                Ok(html) => {
                    let mut response = (status, html).into_response();
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/html; charset=utf-8"),
                    );
                    return Some(response);
                }
                Err(render_error) if missing_view_template(&render_error) => {}
                Err(render_error) => {
                    self.report_error(&render_error);
                    return None;
                }
            }
        }
        None
    }
}

#[cfg(feature = "view")]
fn missing_view_template(error: &Error) -> bool {
    error
        .source()
        .and_then(|source| source.downcast_ref::<minijinja::Error>())
        .is_some_and(|source| source.kind() == minijinja::ErrorKind::TemplateNotFound)
}

fn typed_source<E: StdError + 'static>(error: &Error) -> Option<&E> {
    let mut current: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(source) = current {
        if let Some(matched) = source.downcast_ref::<E>() {
            return Some(matched);
        }
        current = source.source();
    }
    None
}

fn builtin_ignored(error: &Error) -> bool {
    matches!(
        error,
        Error::BadRequest(_)
            | Error::Unauthorized
            | Error::Forbidden(_)
            | Error::NotFound(_)
            | Error::Validation(_)
            | Error::ValidationDetail { .. }
            | Error::ValidationInBag { .. }
            | Error::RateLimited
            | Error::RateLimitedWithRetryAfter(_)
            | Error::HttpWithSource { .. }
    )
}

fn debug_sources(error: &Error) -> Vec<String> {
    let mut sources = Vec::new();
    let mut current = error.source();
    while let Some(source) = current {
        if sources.len() == 16 {
            break;
        }
        sources.push(source.to_string().chars().take(2048).collect());
        current = source.source();
    }
    sources
}

pub(crate) fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Provider 贡献的异常配置；仅保存在当前应用。
#[derive(Default)]
pub(crate) struct ExceptionRegistry(pub(crate) ExceptionHandler);

impl BootContext<'_> {
    /// 沿 Provider boot 顺序配置当前应用的异常 Handler。
    pub fn exceptions<F>(&mut self, configure: F) -> Result<&mut Self>
    where
        F: FnOnce(&mut ExceptionHandler),
    {
        configure(&mut self.extension_mut::<ExceptionRegistry>()?.0);
        Ok(self)
    }
}

impl Application {
    /// 返回本应用 Provider 已登记的异常 Handler；未登记时返回 `None`。
    ///
    /// 外部调用方可复用本应用的翻译目录与异常策略，不另建默认 Handler。
    pub fn exception_handler(&self) -> Option<ExceptionHandler> {
        self.extension::<ExceptionRegistry>()
            .map(|registry| registry.0.clone())
    }

    /// 使用本应用 Provider 登记的报告策略处理 CLI、队列或调度器异常。
    pub fn report_exception(&self, error: &Error) {
        self.exception_handler()
            .unwrap_or_default()
            .report_error(error);
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Debug)]
    struct PrivateFailure;
    impl std::fmt::Display for PrivateFailure {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("credential-like private detail")
        }
    }
    impl StdError for PrivateFailure {}

    fn request(expects_json: bool, debug: bool) -> ExceptionRequest {
        ExceptionRequest::new(Method::GET, "/web", expects_json, "test-id", debug)
    }

    #[test]
    fn source_type_dispatch_short_circuit_and_response_finalizer() {
        let reports = Arc::new(AtomicUsize::new(0));
        let mut handler = ExceptionHandler::default();
        let counter = Arc::clone(&reports);
        handler
            .report::<PrivateFailure, _>(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                ReportDecision::Stop
            })
            .render::<PrivateFailure, _>(|_, _| None)
            .render::<PrivateFailure, _>(|_, _| {
                Some((StatusCode::CONFLICT, "typed").into_response())
            })
            .respond(|mut response, _, _| {
                response
                    .headers_mut()
                    .insert("x-rendered", HeaderValue::from_static("yes"));
                response
            });
        let response =
            handler.render_for_request(Error::internal(PrivateFailure), &request(true, false));
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(response.headers()["x-rendered"], "yes");
        assert_eq!(reports.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn default_html_and_json_hide_internal_source_without_debug() {
        let handler = ExceptionHandler::default();
        let html =
            handler.render_for_request(Error::internal(PrivateFailure), &request(false, false));
        assert_eq!(html.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            html.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        let json =
            handler.render_for_request(Error::internal(PrivateFailure), &request(true, false));
        assert_eq!(json.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json.headers()[header::CONTENT_TYPE], "application/json");
    }

    #[test]
    fn map_and_ignore_use_actual_type() {
        let mut handler = ExceptionHandler::default();
        handler
            .map::<PrivateFailure, _>(|_| Error::NotFound("mapped".into()))
            .dont_report::<Error>();
        let response =
            handler.render_for_request(Error::internal(PrivateFailure), &request(true, false));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn typed_http_source_can_be_rendered_and_builtin_ignore_can_be_overridden()
    -> std::result::Result<(), Box<dyn StdError>> {
        let error = Error::http_with_source(
            StatusCode::from_u16(419)?,
            "page_expired",
            "Page Expired",
            PrivateFailure,
        );
        let mut handler = ExceptionHandler::default();
        assert!(!handler.should_report(&error));
        handler
            .stop_ignoring::<PrivateFailure>()
            .dont_retry::<PrivateFailure>()
            .render::<PrivateFailure, _>(|_, _| {
                Some((StatusCode::CONFLICT, "typed-http").into_response())
            });
        assert!(handler.should_report(&error));
        assert!(handler.should_stop_retries(&error));
        let response = handler.render_for_request(error, &request(false, false));
        assert_eq!(response.status(), StatusCode::CONFLICT);
        Ok(())
    }

    #[test]
    fn custom_client_error_is_reportable_unless_application_ignores_it() {
        let error = Error::custom_with_status(StatusCode::CONFLICT, "conflict", "conflict");
        let mut handler = ExceptionHandler::default();
        assert!(handler.should_report(&error));
        handler.dont_report_when(|error| error.code() == "conflict");
        assert!(!handler.should_report(&error));
    }

    #[test]
    fn report_limit_counts_per_application_instance() {
        let mut first = ExceptionHandler::default();
        let mut second = ExceptionHandler::default();
        for handler in [&mut first, &mut second] {
            handler.throttle::<PrivateFailure, _>(|_| {
                Some(ReportThrottle::Limit {
                    key: "private-failure".to_owned(),
                    max_attempts: 1,
                    window: Duration::from_secs(30),
                })
            });
        }
        let error = Error::internal(PrivateFailure);
        assert!(first.should_report(&error));
        assert!(!first.should_report(&error));
        assert!(second.should_report(&error));
    }

    #[test]
    fn shared_error_identity_is_reported_once() {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        let mut handler = ExceptionHandler::default();
        handler
            .dont_report_duplicates()
            .report::<PrivateFailure, _>(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                ReportDecision::Stop
            });
        let error = Arc::new(Error::internal(PrivateFailure));
        handler.report_shared(&error);
        handler.report_shared(&error);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[cfg(feature = "session")]
    #[test]
    fn web_validation_redirects_before_session_save_and_filters_password_input() -> Result<()> {
        let session = crate::session::Session::new(
            None,
            Arc::new(crate::session::MemorySessionStore::default()),
            None,
        )?;
        session.set_previous_url("/form")?;
        let request = request(false, false);
        request.bind_session(session.clone());
        request.bind_input(serde_json::json!({
            "name": "bad",
            "password": "secret",
            "private_note": "sensitive",
            "profile": {"api_key": "nested-secret", "name": "demo"},
            "items": [{"token": "array-secret", "name": "first"}],
            "literal.dot": "literal-secret"
        }));
        let mut errors = crate::core::ValidationErrors::new();
        errors.add("name", "Invalid name");
        let mut handler = ExceptionHandler::default();
        handler.dont_flash([
            "private_note",
            "profile.api_key",
            "items.0.token",
            "literal.dot",
        ]);
        let response = handler.render_for_request(Error::Validation(errors), &request);
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(response.headers()[header::LOCATION], "/form");
        assert_eq!(
            session.get_json("_old_input")?,
            Some(serde_json::json!({
                "name": "bad",
                "profile": {"name": "demo"},
                "items": [{"name": "first"}]
            }))
        );
        Ok(())
    }
    #[cfg(feature = "session")]
    #[test]
    fn named_validation_uses_the_same_web_error_bag_and_json_fields() -> Result<()> {
        let session = crate::session::Session::new(
            None,
            Arc::new(crate::session::MemorySessionStore::default()),
            None,
        )?;
        session.set_previous_url("/two-factor")?;
        let request = request(false, false);
        request.bind_session(session.clone());
        let mut errors = crate::core::ValidationErrors::new();
        errors.add("code", "invalid");
        let handler = ExceptionHandler::default();
        let response = handler.render_for_request(
            Error::validation_in_bag("confirmTwoFactorAuthentication", errors),
            &request,
        );
        assert_eq!(response.status(), StatusCode::FOUND);
        let bags = session.read_errors()?;
        assert!(bags.has_bag("confirmTwoFactorAuthentication"));
        assert!(!bags.has_bag("default"));
        Ok(())
    }
    #[cfg(feature = "auth-fortify")]
    #[tokio::test]
    async fn selected_credential_error_cache_policy_uses_route_prefix_semantics()
    -> Result<(), Box<dyn StdError>> {
        let mut handler = ExceptionHandler::default();
        handler.respond(|mut response, _, _| {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=60"),
            );
            response
        });
        let unselected = handler.clone();
        handler.no_store_errors_under("/api/{tenant}/auth".into());
        handler.no_store_errors_under("/jwt/auth".into());
        for path in [
            "/api/acme/auth/login",
            "/api/acme/auth",
            "/jwt/auth",
            "/jwt/auth/user",
        ] {
            for method in [Method::GET, Method::HEAD] {
                let request =
                    ExceptionRequest::new(method.clone(), path, true, "cache-policy", false);
                let response = handler.render_for_request(Error::Unauthorized, &request);
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                assert_eq!(response.headers()[header::PRAGMA], "no-cache");
                if method == Method::HEAD {
                    assert!(
                        axum::body::to_bytes(response.into_body(), 1024)
                            .await?
                            .is_empty()
                    );
                }
                let response = unselected.render_for_request(Error::Unauthorized, &request);
                assert_eq!(
                    response.headers()[header::CACHE_CONTROL],
                    "public, max-age=60"
                );
                assert!(response.headers().get(header::PRAGMA).is_none());
            }
        }
        for path in [
            "/api/acme/authentication",
            "/apis/acme/auth",
            "/api//auth/login",
            "/jwt/authentication",
            "/web/login",
        ] {
            let request = ExceptionRequest::new(Method::GET, path, true, "cache-policy", false);
            let response = handler.render_for_request(Error::Unauthorized, &request);
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "public, max-age=60"
            );
            assert!(response.headers().get(header::PRAGMA).is_none());
        }
        Ok(())
    }
}
