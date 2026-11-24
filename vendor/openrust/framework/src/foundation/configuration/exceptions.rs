use std::{any::TypeId, error::Error, sync::Arc};

use crate::{
    cache::rate_limiting::Limit,
    contracts::debug::{Exception, SharedException},
    http::{HttpError, HttpFuture, Request, Response},
    log::{Level, LogContext},
    support::Lottery,
};

type JsonPolicy = dyn Fn(&Request, &Exception<'_>) -> bool + Send + Sync;
type Throttle =
    dyn Fn(&Exception<'_>) -> Option<(ReportThrottle, Option<&'static str>)> + Send + Sync;
type Reporter = dyn Fn(&Exception<'_>) -> ReportDecision + Send + Sync;
type Renderer = dyn Fn(&Exception<'_>, &Request) -> Option<Response> + Send + Sync;
type AsyncRenderer = dyn for<'a, 'e> Fn(
        &'a Exception<'e>,
        &'a mut Request,
    ) -> HttpFuture<'a, Result<Option<Response>, HttpError>>
    + Send
    + Sync;
#[derive(Clone)]
enum RendererEntry {
    Sync(Arc<Renderer>),
    Async(Arc<AsyncRenderer>),
}
type Responder = dyn Fn(Response, &Exception<'_>, &Request) -> Response + Send + Sync;

type AsyncResponder = dyn for<'a, 'e> Fn(Response, &'a Exception<'e>, &'a mut Request) -> HttpFuture<'a, Response>
    + Send
    + Sync;
#[derive(Clone)]
enum ResponderEntry {
    Sync(Arc<Responder>),
    Async(Arc<AsyncResponder>),
}

type Mapper = dyn Fn(&Exception<'_>) -> Option<SharedException> + Send + Sync;
#[derive(Clone)]
struct Mapping {
    kind: TypeId,
    callback: Arc<Mapper>,
}

type Matcher = dyn Fn(&Exception<'_>) -> bool + Send + Sync;
type Context = dyn Fn(&Exception<'_>, &LogContext) -> LogContext + Send + Sync;
type TypedContext = dyn Fn(&(dyn Error + 'static)) -> LogContext + Send + Sync;

#[derive(Clone)]
struct ErrorContext {
    name: &'static str,
    matches: fn(&(dyn Error + 'static)) -> bool,
    context: Arc<TypedContext>,
}

#[derive(Clone)]
struct Rule {
    kind: TypeId,
    matches: Arc<Matcher>,
}

impl Rule {
    fn typed<E: Error + 'static>() -> Self {
        Self {
            kind: TypeId::of::<E>(),
            matches: Arc::new(|exception| exception.downcast_ref::<E>().is_some()),
        }
    }
}

/// Controls whether later callbacks and default reporting run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportDecision {
    Continue,
    Stop,
}

/// Determines whether an otherwise reportable exception is logged.
#[derive(Clone)]
pub enum ReportThrottle {
    Unlimited,
    Lottery(Lottery),
    Limit(Limit),
}

/// Configures exception reporting and HTTP rendering.
#[derive(Clone, Default)]
pub struct Exceptions {
    json_policy: Option<Arc<JsonPolicy>>,
    mappings: Vec<Mapping>,
    inner_exceptions: Vec<Mapping>,
    reporters: Vec<Arc<Reporter>>,
    exception_types: Vec<TypeId>,
    exception_reporters: Vec<Arc<Reporter>>,
    exception_renderers: Vec<Arc<Renderer>>,
    exception_async_renderers: Vec<Arc<AsyncRenderer>>,
    behavior_context: Vec<Arc<Context>>,
    throttles: Vec<Arc<Throttle>>,
    ignored: Vec<Rule>,
    unignored: Vec<Rule>,
    ignore_when: Vec<Arc<Matcher>>,
    stop_retry_types: Vec<Rule>,
    stop_retry_when: Vec<Arc<Matcher>>,
    levels: Vec<(Rule, Level)>,
    context: Vec<Arc<Context>>,
    exception_context: Vec<ErrorContext>,
    without_duplicates: bool,
    renderers: Vec<RendererEntry>,
    responder: Option<ResponderEntry>,
    dont_flash: Vec<String>,
}

impl Exceptions {
    /// 注册异常类型自身的方法；显式元数据替代可选方法反射，不改变实际错误实例。
    pub fn use_exception<E: crate::contracts::debug::ExceptionBehavior>(&mut self) -> &mut Self {
        if self.exception_types.contains(&TypeId::of::<E>()) {
            return self;
        }
        self.exception_types.push(TypeId::of::<E>());
        self.exception_reporters.push(Arc::new(|exception| {
            exception
                .downcast_ref::<E>()
                .map_or(ReportDecision::Continue, E::report)
        }));
        self.exception_renderers
            .push(Arc::new(|exception, request| {
                exception
                    .downcast_ref::<E>()
                    .and_then(|error| error.render(request))
            }));
        self.exception_async_renderers
            .push(Arc::new(|exception, request| {
                match exception.downcast_ref::<E>() {
                    Some(error) => error.render_async(request),
                    None => Box::pin(async { Ok(None) }),
                }
            }));
        // 类型名复用原诊断注册；实例context仅在报告上下文构造时调用一次。
        self.exception_context::<E>(|_| LogContext::new());
        self.behavior_context.push(Arc::new(|exception, _| {
            exception
                .downcast_ref::<E>()
                .map_or_else(LogContext::new, E::context)
        }));
        self
    }

    /// 同类型重新注册覆盖原回调并保留其顺序；只应用首个匹配映射。
    pub fn map<E, M>(&mut self, callback: impl Fn(&E) -> M + Send + Sync + 'static) -> &mut Self
    where
        E: Error + 'static,
        M: Error + Send + Sync + 'static,
    {
        self.map_optional(move |error| Some(callback(error)))
    }
    /// 可选映射用于枚举变体；None 保留原错误并继续检查其余映射。
    pub fn map_optional<E, M>(
        &mut self,
        callback: impl Fn(&E) -> Option<M> + Send + Sync + 'static,
    ) -> &mut Self
    where
        E: Error + 'static,
        M: Error + Send + Sync + 'static,
    {
        let mapping = Mapping {
            kind: TypeId::of::<E>(),
            callback: Arc::new(move |exception| {
                exception
                    .downcast_ref::<E>()
                    .and_then(&callback)
                    .map(SharedException::new)
            }),
        };
        match self
            .mappings
            .iter_mut()
            .find(|entry| entry.kind == mapping.kind)
        {
            Some(entry) => *entry = mapping,
            None => self.mappings.push(mapping),
        }
        self
    }
    /// 为 Rust 错误显式接入可选的 getInnerException 等价入口，优先于普通映射。
    pub fn inner_exception<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E) -> Option<SharedException> + Send + Sync + 'static,
    ) -> &mut Self {
        let mapping = Mapping {
            kind: TypeId::of::<E>(),
            callback: Arc::new(move |exception| exception.downcast_ref::<E>().and_then(&callback)),
        };
        match self
            .inner_exceptions
            .iter_mut()
            .find(|entry| entry.kind == mapping.kind)
        {
            Some(entry) => *entry = mapping,
            None => self.inner_exceptions.push(mapping),
        }
        self
    }
    pub(crate) fn map_exception(&self, exception: &Exception<'_>) -> Option<SharedException> {
        if matches!(exception, Exception::Mapped(_)) {
            return None;
        }
        self.inner_exceptions
            .iter()
            .chain(&self.mappings)
            .find_map(|entry| (entry.callback)(exception))
    }

    /// Registers a report callback for an exception type or one of its sources.
    pub fn report<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E) -> ReportDecision + Send + Sync + 'static,
    ) -> &mut Self {
        self.reportable(callback);
        self
    }

    pub fn reportable<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E) -> ReportDecision + Send + Sync + 'static,
    ) -> crate::foundation::exceptions::ReportableHandler {
        self.register_reportable(crate::foundation::exceptions::ReportableHandler::for_type(
            callback,
        ))
    }
    pub fn reportable_all(
        &mut self,
        callback: impl Fn(&Exception<'_>) -> ReportDecision + Send + Sync + 'static,
    ) -> crate::foundation::exceptions::ReportableHandler {
        self.register_reportable(crate::foundation::exceptions::ReportableHandler::new(
            callback,
        ))
    }
    fn register_reportable(
        &mut self,
        handler: crate::foundation::exceptions::ReportableHandler,
    ) -> crate::foundation::exceptions::ReportableHandler {
        let registered = handler.clone();
        self.reporters.push(Arc::new(move |exception| {
            match registered.invoke(exception) {
                Ok(false) => ReportDecision::Stop,
                Ok(true) | Err(_) => ReportDecision::Continue,
            }
        }));
        handler
    }

    /// Sets a sampling or rate limit policy for matching exception reports.
    pub fn throttle<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E) -> Option<ReportThrottle> + Send + Sync + 'static,
    ) -> &mut Self {
        self.throttles.push(Arc::new(move |exception| {
            exception
                .downcast_ref::<E>()
                .and_then(&callback)
                .map(|policy| {
                    (
                        policy,
                        exception.type_name().or(Some(std::any::type_name::<E>())),
                    )
                })
        }));
        self
    }

    /// 按注册顺序处理所有异常；None继续，Unlimited明确停止策略查找。
    pub fn throttle_all(
        &mut self,
        callback: impl Fn(&Exception<'_>) -> Option<ReportThrottle> + Send + Sync + 'static,
    ) -> &mut Self {
        self.throttles.push(Arc::new(move |exception| {
            callback(exception).map(|policy| (policy, exception.type_name()))
        }));
        self
    }

    pub(crate) fn report_throttle(
        &self,
        exception: &Exception<'_>,
    ) -> Option<(ReportThrottle, Option<&'static str>)> {
        self.throttles
            .iter()
            .find_map(|throttle| throttle(exception))
    }

    /// Sets the default log level for a matching exception type.
    pub fn level<E: Error + 'static>(&mut self, level: Level) -> &mut Self {
        let rule = Rule::typed::<E>();
        if let Some((_, configured)) = self
            .levels
            .iter_mut()
            .find(|(existing, _)| existing.kind == rule.kind)
        {
            *configured = level;
        } else {
            self.levels.push((rule, level));
        }
        self
    }

    /// Adds fields to the default exception log record.
    pub fn context(
        &mut self,
        callback: impl Fn(&Exception<'_>) -> LogContext + Send + Sync + 'static,
    ) -> &mut Self {
        self.context
            .push(Arc::new(move |exception, _| callback(exception)));
        self
    }

    /// 逐个回调接收此前累积字段；后返回的同名字段覆盖之前的值。
    pub fn context_with(
        &mut self,
        callback: impl Fn(&Exception<'_>, &LogContext) -> LogContext + Send + Sync + 'static,
    ) -> &mut Self {
        self.context.push(Arc::new(callback));
        self
    }

    /// Adds log context for this exact error type, including when it is a source.
    pub fn exception_context<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E) -> LogContext + Send + Sync + 'static,
    ) -> &mut Self {
        self.exception_context.push(ErrorContext {
            name: std::any::type_name::<E>(),
            matches: |error| error.is::<E>(),
            context: Arc::new(move |error| {
                callback(error.downcast_ref::<E>().expect("matched error type"))
            }),
        });
        self
    }

    pub fn dont_report<E: Error + 'static>(&mut self) -> &mut Self {
        self.ignored.push(Rule::typed::<E>());
        self
    }

    pub fn stop_ignoring<E: Error + 'static>(&mut self) -> &mut Self {
        let rule = Rule::typed::<E>();
        self.ignored.retain(|existing| existing.kind != rule.kind);
        self.unignored.push(rule);
        self
    }

    pub fn dont_report_when(
        &mut self,
        callback: impl Fn(&Exception<'_>) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.ignore_when.push(Arc::new(callback));
        self
    }

    /// Stops queued job retries for the given exception type or one of its sources.
    pub fn dont_retry<E: Error + 'static>(&mut self) -> &mut Self {
        let rule = Rule::typed::<E>();
        if !self
            .stop_retry_types
            .iter()
            .any(|existing| existing.kind == rule.kind)
        {
            self.stop_retry_types.push(rule);
        }
        self
    }

    /// Stops queued job retries when the predicate matches.
    pub fn dont_retry_when(
        &mut self,
        callback: impl Fn(&Exception<'_>) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.stop_retry_when.push(Arc::new(callback));
        self
    }

    pub(crate) fn stops_retries(&self, exception: &Exception<'_>) -> bool {
        self.stop_retry_types
            .iter()
            .any(|rule| (rule.matches)(exception))
            || self
                .stop_retry_when
                .iter()
                .any(|matches| matches(exception))
    }

    pub fn dont_report_duplicates(&mut self) -> &mut Self {
        self.without_duplicates = true;
        self
    }

    pub(crate) fn suppress_duplicates(&self) -> bool {
        self.without_duplicates
    }

    pub(crate) fn allows_reporting(&self, exception: &Exception<'_>, default: bool) -> bool {
        (default || self.unignored.iter().any(|rule| (rule.matches)(exception)))
            && !self.ignored.iter().any(|rule| (rule.matches)(exception))
            && !self
                .ignore_when
                .iter()
                .any(|predicate| predicate(exception))
    }

    pub(crate) fn report_level(&self, exception: &Exception<'_>) -> Level {
        self.levels
            .iter()
            .find_map(|(rule, level)| (rule.matches)(exception).then_some(*level))
            .unwrap_or(Level::Error)
    }

    pub(crate) fn report_context(&self, exception: &Exception<'_>) -> LogContext {
        let mut context = self.node_context(exception.root_error()).1;
        for callback in &self.behavior_context {
            context.extend(callback(exception, &context));
        }
        for callback in &self.context {
            context.extend(callback(exception, &context));
        }
        context
    }

    pub(crate) fn node_context(
        &self,
        error: &(dyn Error + 'static),
    ) -> (Option<&'static str>, LogContext) {
        let mut name = None;
        let mut context = LogContext::new();
        for configuration in self
            .exception_context
            .iter()
            .filter(|entry| (entry.matches)(error))
        {
            name = Some(configuration.name);
            context.extend((configuration.context)(error));
        }
        (name, context)
    }

    pub(crate) fn node_name(&self, error: &(dyn Error + 'static)) -> Option<&'static str> {
        self.exception_context
            .iter()
            .find(|entry| (entry.matches)(error))
            .map(|entry| entry.name)
    }

    /// Registers an HTTP renderer. Returning `None` leaves the error to the next renderer.
    pub fn render<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E, &Request) -> Option<Response> + Send + Sync + 'static,
    ) -> &mut Self {
        self.renderers
            .push(RendererEntry::Sync(Arc::new(move |exception, request| {
                exception
                    .downcast_ref::<E>()
                    .and_then(|error| callback(error, request))
            })));
        self
    }

    /// 异步HTTP渲染器，与同步渲染器按注册顺序执行；None继续，错误交回异常边界。
    pub fn render_async<E: Error + Sync + 'static>(
        &mut self,
        callback: impl for<'a> Fn(
            &'a E,
            &'a mut Request,
        ) -> HttpFuture<'a, Result<Option<Response>, HttpError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.renderers.push(RendererEntry::Async(Arc::new(
            move |exception, request| match exception.downcast_ref::<E>() {
                Some(error) => callback(error, request),
                None => Box::pin(async { Ok(None) }),
            },
        )));
        self
    }

    /// Sets the final callback for custom and default HTTP exception responses.
    pub fn respond(
        &mut self,
        callback: impl Fn(Response, &Exception<'_>, &Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        self.responder = Some(ResponderEntry::Sync(Arc::new(callback)));
        self
    }

    /// 在最终响应阶段借用可变请求，允许异步转换和Session闪存；后注册者替换前者。
    pub fn respond_async(
        &mut self,
        callback: impl for<'a, 'e> Fn(
            Response,
            &'a Exception<'e>,
            &'a mut Request,
        ) -> HttpFuture<'a, Response>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.responder = Some(ResponderEntry::Async(Arc::new(callback)));
        self
    }

    /// Excludes additional input paths from validation redirects.
    pub fn dont_flash(&mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        self.dont_flash.extend(fields.into_iter().map(Into::into));
        self
    }

    pub(crate) fn flash_exclusions(&self) -> &[String] {
        &self.dont_flash
    }

    /// Overrides the default request-based JSON decision.
    pub fn should_render_json_when(
        &mut self,
        policy: impl Fn(&Request, &Exception<'_>) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.json_policy = Some(Arc::new(policy));
        self
    }

    pub(crate) fn renders_json(&self, request: &Request, exception: &Exception<'_>) -> bool {
        self.json_policy.as_ref().map_or_else(
            || request.expects_json(),
            |policy| policy(request, exception),
        )
    }

    pub(crate) fn should_continue_reporting(&self, exception: &Exception<'_>) -> bool {
        self.exception_reporters
            .iter()
            .chain(&self.reporters)
            .all(|report| report(exception) == ReportDecision::Continue)
    }

    pub(crate) fn render_using(
        &self,
        exception: &Exception<'_>,
        request: &Request,
    ) -> Option<Response> {
        self.exception_renderers
            .iter()
            .chain(self.renderers.iter().filter_map(|entry| match entry {
                RendererEntry::Sync(render) => Some(render),
                RendererEntry::Async(_) => None,
            }))
            .find_map(|render| render(exception, request))
    }

    pub(crate) async fn render_using_async(
        &self,
        exception: &Exception<'_>,
        request: &mut Request,
    ) -> Result<Option<Response>, HttpError> {
        for render in &self.exception_async_renderers {
            if let Some(response) = render(exception, request).await? {
                return Ok(Some(response));
            }
        }
        for entry in &self.renderers {
            let response = match entry {
                RendererEntry::Sync(render) => render(exception, request),
                RendererEntry::Async(render) => render(exception, request).await?,
            };
            if response.is_some() {
                return Ok(response);
            }
        }
        Ok(None)
    }

    pub(crate) async fn respond_using_async(
        &self,
        response: Response,
        exception: &Exception<'_>,
        request: &mut Request,
    ) -> Response {
        match &self.responder {
            Some(ResponderEntry::Sync(respond)) => respond(response, exception, request),
            Some(ResponderEntry::Async(respond)) => respond(response, exception, request).await,
            None => response,
        }
    }

    pub(crate) fn respond_using(
        &self,
        response: Response,
        exception: &Exception<'_>,
        request: &Request,
    ) -> Response {
        match &self.responder {
            Some(ResponderEntry::Sync(respond)) => respond(response, exception, request),
            Some(ResponderEntry::Async(_)) | None => response,
        }
    }
}
