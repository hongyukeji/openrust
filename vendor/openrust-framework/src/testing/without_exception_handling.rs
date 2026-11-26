use super::ExceptionType;
use crate::{
    console::Output,
    contracts::debug::{Exception, ExceptionHandler, SharedException},
    http::{HttpError, HttpFuture, Request, Response},
    log::LogResult,
};
use std::{error::Error, fmt, sync::Arc};

/// 关闭处理时补充404请求信息，保留原始404作为来源，不复制错误快照。
#[derive(Debug)]
struct RequestNotFound {
    method: crate::http::Method,
    url: String,
    previous: SharedException,
}
impl fmt::Display for RequestNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.method, self.url)
    }
}
impl Error for RequestNotFound {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.previous)
    }
}

pub(crate) struct WithoutExceptionHandling {
    original: Arc<dyn ExceptionHandler>,
    except: Vec<ExceptionType>,
}
impl WithoutExceptionHandling {
    pub(crate) fn new(original: Arc<dyn ExceptionHandler>, except: Vec<ExceptionType>) -> Self {
        Self { original, except }
    }
    fn handles(&self, exception: &Exception<'_>) -> bool {
        self.except.iter().any(|kind| kind.matches(exception))
    }
    fn owned(exception: &Exception<'_>) -> SharedException {
        SharedException::from_exception(exception).unwrap_or_else(SharedException::new)
    }
    fn describe(request: &Request, error: SharedException) -> SharedException {
        if matches!(
            Exception::Error(&error)
                .root_error()
                .downcast_ref::<HttpError>(),
            Some(HttpError::NotFound)
        ) {
            SharedException::new(RequestNotFound {
                method: request.method().clone(),
                url: request
                    .url()
                    .unwrap_or_else(|_| request.uri().path().to_owned()),
                previous: error,
            })
        } else {
            error
        }
    }
}
impl ExceptionHandler for WithoutExceptionHandling {
    fn is_without_exception_handling(&self) -> bool {
        true
    }
    fn should_report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async { false })
    }
    fn report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn handle_http<'a>(
        &'a self,
        request: &'a mut Request,
        error: Arc<HttpError>,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move {
            if self.handles(&Exception::Http(&error)) {
                return self.original.render_http_response(request, &error).await;
            }
            let error = Self::describe(request, SharedException::from_arc(error));
            let _ = super::exception_handler_fake::escape_report(error.clone()).await;
            // 原始Kernel返回Response；测试工具通过捕获入口返回Result，直接调用保留显式重抛。
            std::panic::panic_any(error)
        })
    }
    fn render_result(
        &self,
        request: &Request,
        exception: &Exception<'_>,
    ) -> Result<Response, SharedException> {
        if self.handles(exception) {
            self.original.render_result(request, exception)
        } else {
            Err(Self::describe(request, Self::owned(exception)))
        }
    }
    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response {
        self.render_result(request, exception)
            .unwrap_or_else(|error| std::panic::panic_any(error))
    }
    fn render_for_console(&self, output: &mut Output, exception: &Exception<'_>) {
        use fmt::Write as _;
        writeln!(output.stderr, "{}", exception.error()).expect("写入内存字符串不会失败");
    }
}
