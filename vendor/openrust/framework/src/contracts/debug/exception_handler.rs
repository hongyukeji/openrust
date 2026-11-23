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
pub struct SharedException(Arc<dyn Error + Send + Sync>);

impl SharedException {
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self(Arc::new(error))
    }

    pub fn from_arc(error: Arc<dyn Error + Send + Sync>) -> Self {
        Self(error)
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
pub enum Exception<'a> {
    Http(&'a HttpError),
    Error(&'a (dyn Error + Send + Sync + 'static)),
}

impl Exception<'_> {
    pub(crate) fn error(&self) -> &(dyn Error + 'static) {
        match self {
            Self::Http(error) => *error,
            Self::Error(error) => *error,
        }
    }

    /// Finds the first matching error in the exception and its source chain.
    pub fn downcast_ref<E: Error + 'static>(&self) -> Option<&E> {
        let mut current = Some(self.error());
        while let Some(error) = current {
            if let Some(matched) = error.downcast_ref::<E>() {
                return Some(matched);
            }
            current = error.source();
        }
        None
    }
}

/// Reports exceptions and renders them at the appropriate application boundary.
pub trait ExceptionHandler: Any + Send + Sync {
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
    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response;
    fn render_for_console(&self, output: &mut Output, exception: &Exception<'_>);
}
