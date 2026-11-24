use std::{
    any::Any,
    error::Error,
    fmt,
    sync::{Arc, Weak},
};

use crate::{
    console::Output,
    http::{HttpError, HttpFuture, Request, Response},
    log::LogResult,
};

/// An error with explicit, shared instance identity for duplicate reporting.
#[derive(Clone, Debug)]
pub struct SharedException(Arc<dyn Error + Send + Sync>, Option<&'static str>, bool);

/// 借用的错误无法凭空取得共享所有权，不能用字符串快照代替原对象。
#[derive(Clone, Copy, Debug)]
pub struct ExceptionOwnershipError;
impl fmt::Display for ExceptionOwnershipError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "此入口需要共享异常所有权；请使用 SharedException 或接收错误所有权的公开入口",
        )
    }
}
impl Error for ExceptionOwnershipError {}

impl SharedException {
    /// 重复传入共享包装时复用原始实例与去重身份。
    pub fn new<E: Error + Send + Sync + 'static>(error: E) -> Self {
        if let Some(shared) = (&error as &(dyn Error + 'static)).downcast_ref::<Self>() {
            shared.clone()
        } else {
            Self(Arc::new(error), Some(std::any::type_name::<E>()), false)
        }
    }

    pub fn from_arc(error: Arc<dyn Error + Send + Sync>) -> Self {
        if let Some(shared) = error.as_ref().downcast_ref::<Self>() {
            shared.clone()
        } else {
            Self(error, None, false)
        }
    }

    /// 保留已知具体类型；已擦除类型的Arc可继续用from_arc并在限流策略显式指定键。
    pub fn from_typed_arc<E: Error + Send + Sync + 'static>(error: Arc<E>) -> Self {
        if let Some(shared) = (error.as_ref() as &(dyn Error + 'static)).downcast_ref::<Self>() {
            shared.clone()
        } else {
            Self(error, Some(std::any::type_name::<E>()), false)
        }
    }

    /// 在原始实例上保留ShouldntReport，不转成消息或重新包装其身份。
    pub fn unreported<E: super::ShouldntReport>(error: E) -> Self {
        let mut shared = Self::new(error);
        shared.2 = true;
        shared
    }

    pub fn from_exception(exception: &Exception<'_>) -> Result<Self, ExceptionOwnershipError> {
        exception
            .error()
            .downcast_ref::<Self>()
            .cloned()
            .ok_or(ExceptionOwnershipError)
    }

    pub(crate) fn identity(&self) -> Weak<dyn Error + Send + Sync> {
        Arc::downgrade(&self.0)
    }
}

impl fmt::Display for SharedException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl Error for SharedException {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Borrows the original error while preserving HTTP-specific metadata.
#[derive(Clone, Copy)]
pub enum Exception<'a> {
    Http(&'a HttpError),
    /// 映射后的业务错误按根类型匹配，来源只保留为诊断链。
    Mapped(&'a SharedException),
    Error(&'a (dyn Error + Send + Sync + 'static)),
    /// 显式借用具体类型，保留按类型分组所需的原生标识。
    Typed(&'a (dyn Error + Send + Sync + 'static), &'static str),
}

impl<'a> Exception<'a> {
    pub fn typed<E: Error + Send + Sync + 'static>(error: &'a E) -> Self {
        Self::Typed(error, std::any::type_name::<E>())
    }

    pub fn type_name(&self) -> Option<&'static str> {
        if let Some(shared) = self.error().downcast_ref::<SharedException>() {
            return shared.1;
        }
        match self {
            Self::Http(_) => Some(std::any::type_name::<HttpError>()),
            Self::Typed(_, name) => Some(name),
            Self::Error(_) | Self::Mapped(_) => None,
        }
    }

    pub(crate) fn error(&self) -> &(dyn Error + 'static) {
        match self {
            Self::Http(error) => *error,
            Self::Mapped(error) => *error,
            Self::Error(error) | Self::Typed(error, _) => *error,
        }
    }

    /// 只穿过框架透明传输包装；不会因业务source含被忽略错误而忽略外层类型。
    pub(crate) fn suppresses_reporting(&self) -> bool {
        let mut error = self.error();
        for _ in 0..32 {
            if let Some(shared) = error.downcast_ref::<SharedException>() {
                if shared.2 {
                    return true;
                }
                error = shared.0.as_ref();
            } else if let Some(HttpError::Internal { source }) = error.downcast_ref::<HttpError>() {
                error = source.as_ref();
            } else {
                return false;
            }
        }
        false
    }

    pub(crate) fn root_error(&self) -> &(dyn Error + 'static) {
        let mut error = self.error();
        while error.is::<SharedException>() {
            error = error.source().expect("共享异常保留原始错误");
        }
        error
    }

    /// 查找原始错误及来源；映射后只匹配新根，来源继续保留为诊断链。
    pub fn downcast_ref<E: Error + 'static>(&self) -> Option<&E> {
        if matches!(self, Self::Mapped(_)) {
            return self.root_error().downcast_ref::<E>();
        }
        self.downcast_chain_ref::<E>()
    }

    /// 显式读取完整诊断链，包括映射前来源；限制深度以免循环source阻塞调用。
    pub fn downcast_chain_ref<E: Error + 'static>(&self) -> Option<&E> {
        let mut current = Some(self.error());
        for _ in 0..32 {
            let error = current?;
            if let Some(matched) = error.downcast_ref::<E>() {
                return Some(matched);
            }
            current = error.source();
        }
        None
    }
}

/// 字段与原始异常分别保留；不把错误对象伪装成 JSON 字段。
pub struct ExceptionContext<'a> {
    pub fields: crate::log::LogContext,
    pub exception: Exception<'a>,
}

fn unsupported_handler_method(
    method: &'static str,
) -> crate::contracts::container::BindingResolutionError {
    crate::contracts::container::BindingResolutionError::FactoryFailed {
        service: "ExceptionHandler",
        source: Box::new(std::io::Error::other(format!(
            "此异常处理器不支持 {method}"
        ))),
    }
}

/// Reports exceptions and renders them at the appropriate application boundary.
pub trait ExceptionHandler: Any + Send + Sync {
    /// 对应测试处理器的标记接口；普通处理器不启用此模式。
    fn is_without_exception_handling(&self) -> bool {
        false
    }
    /// 标准处理器明确实现空注册；自定义处理器不可伪造能力成功。
    fn register(&self) -> Result<(), crate::contracts::container::BindingResolutionError> {
        Err(unsupported_handler_method("register"))
    }
    fn build_context_for_exception(
        &self,
        _exception: &Exception<'_>,
    ) -> Result<crate::log::LogContext, crate::contracts::container::BindingResolutionError> {
        Err(unsupported_handler_method("build_context_for_exception"))
    }
    fn context_for_exception<'a, 'e>(
        &'a self,
        _exception: &'a Exception<'e>,
    ) -> HttpFuture<
        'a,
        Result<ExceptionContext<'e>, crate::contracts::container::BindingResolutionError>,
    > {
        Box::pin(async { Err(unsupported_handler_method("context_for_exception")) })
    }
    /// 可变请求由调用方显式借出，支持在错误边界安全执行惰性认证。
    fn context_for_exception_with_request<'a, 'e>(
        &'a self,
        _request: &'a mut Request,
        exception: &'a Exception<'e>,
    ) -> HttpFuture<
        'a,
        Result<ExceptionContext<'e>, crate::contracts::container::BindingResolutionError>,
    > {
        self.context_for_exception(exception)
    }
    /// 同步修改运行期配置；自定义处理器须显式支持，不能静默忽略。
    fn configure(
        &self,
        _update: &mut dyn FnMut(&mut crate::foundation::configuration::Exceptions),
    ) -> Result<(), crate::contracts::container::BindingResolutionError> {
        Err(
            crate::contracts::container::BindingResolutionError::FactoryFailed {
                service: "ExceptionHandler",
                source: Box::new(std::io::Error::other("此异常处理器不支持运行期配置")),
            },
        )
    }
    /// 查询同一异常实例是否正在报告；None 表示自定义处理器未公开此能力。
    fn is_reporting(&self, _exception: &Exception<'_>) -> Option<bool> {
        None
    }
    /// Builds a serializable snapshot of the original exception and its sources.
    fn build_log_exception(
        &self,
        exception: &Exception<'_>,
        name: Option<&'static str>,
        _reported: bool,
    ) -> serde_json::Value {
        crate::log::json::snapshot(
            exception,
            name,
            |_| (None, Default::default()),
            Default::default(),
        )
    }

    /// Determines whether a queued attempt should fail instead of being retried.
    fn should_stop_retries(&self, _exception: &Exception<'_>) -> bool {
        false
    }

    /// 检查报告资格，包含采样与限流准入；重复查询也会执行这些策略。
    fn should_report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, bool>;
    fn report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>>;
    /// 明确借出请求，供需要惰性认证上下文的报告调用使用。
    fn report_with_request<'a>(
        &'a self,
        _request: &'a mut Request,
        exception: &'a Exception<'_>,
    ) -> HttpFuture<'a, LogResult<()>> {
        self.report(exception)
    }
    /// 默认扩展路径同时报告和渲染；具体处理器可保留请求级重定向与日志上下文。
    fn render_http<'a>(
        &'a self,
        request: &'a mut Request,
        error: &'a HttpError,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move {
            let exception = Exception::Http(error);
            let _ = self.report(&exception).await;
            self.render(request, &exception)
        })
    }
    /// 在拥有错误的边界保留原始实例，供测试替身安全记录。
    fn handle_http<'a>(
        &'a self,
        request: &'a mut Request,
        error: Arc<HttpError>,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move { self.render_http(request, &error).await })
    }
    /// 仅渲染，不再次报告；保留具体处理器的验证重定向和响应钩子。
    fn render_http_response<'a>(
        &'a self,
        request: &'a mut Request,
        error: &'a HttpError,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move { self.render(request, &Exception::Http(error)) })
    }
    /// 保留可失败的原生渲染入口，关闭处理时仍可用Result传出原始共享错误。
    fn render_result(
        &self,
        request: &Request,
        exception: &Exception<'_>,
    ) -> Result<Response, SharedException> {
        Ok(self.render(request, exception))
    }
    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response;
    fn render_for_console(&self, output: &mut Output, exception: &Exception<'_>);
}
