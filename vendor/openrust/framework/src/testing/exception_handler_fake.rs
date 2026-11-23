use crate::{
    console::Output,
    contracts::{
        container::BindingResolutionError,
        debug::{Exception, ExceptionHandler, SharedException},
    },
    foundation::configuration::Exceptions,
    http::{HttpError, HttpFuture, Request, Response},
    log::{LogError, LogResult},
};
use std::{
    error::Error,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

tokio::task_local! {
    static REPORT_ESCAPE: Mutex<Option<tokio::sync::oneshot::Sender<SharedException>>>;
}

/// 只中止当前受保护调用；不把测试控制信号泄漏到并发任务或外层调用。
pub(crate) async fn catch_reported<T>(
    future: impl std::future::Future<Output = T>,
) -> Result<T, SharedException> {
    let (sender, mut receiver) = tokio::sync::oneshot::channel();
    REPORT_ESCAPE
        .scope(Mutex::new(Some(sender)), async move {
            tokio::select! {
                biased;
                error = &mut receiver => Err(error.expect("报告通道随调用作用域存活")),
                value = future => Ok(value),
            }
        })
        .await
}

async fn escape_report(error: SharedException) -> LogResult<()> {
    let captured = REPORT_ESCAPE
        .try_with(|slot| {
            slot.lock()
                .unwrap()
                .take()
                .is_some_and(|sender| sender.send(error.clone()).is_ok())
        })
        .unwrap_or(false);
    if captured {
        // 受保护入口收到错误后丢弃本次 Future，报告后续代码不会继续执行。
        std::future::pending::<LogResult<()>>().await
    } else {
        Err(LogError::WriteFailed(Box::new(error)))
    }
}

fn root_error<'a>(error: &'a (dyn Error + 'static)) -> &'a (dyn Error + 'static) {
    let mut error = error;
    while error.is::<SharedException>() {
        error = error.source().expect("共享异常保留原始错误");
    }
    error
}

type Matcher = dyn Fn(&Exception<'_>) -> bool + Send + Sync;

/// 精确匹配报告根类型；需要检查传输包装的源链时显式选择 including_sources。
#[derive(Clone)]
pub struct ExceptionType {
    matches: Arc<Matcher>,
}
impl ExceptionType {
    pub fn of<E: Error + 'static>() -> Self {
        Self {
            matches: Arc::new(|exception| root_error(exception.error()).is::<E>()),
        }
    }
    pub fn including_sources<E: Error + 'static>() -> Self {
        Self {
            matches: Arc::new(|exception| exception.downcast_ref::<E>().is_some()),
        }
    }
    fn matches(&self, exception: &Exception<'_>) -> bool {
        (self.matches)(exception)
    }
}

/// 记录真实拥有所有权的错误；不把类型和源链转换为字符串快照。
pub struct ExceptionHandlerFake {
    handler: RwLock<Arc<dyn ExceptionHandler>>,
    exceptions: Vec<ExceptionType>,
    reported: Mutex<Vec<SharedException>>,
    throw_on_report: AtomicBool,
}
impl ExceptionHandlerFake {
    pub fn new(handler: Arc<dyn ExceptionHandler>, exceptions: Vec<ExceptionType>) -> Self {
        Self {
            handler: RwLock::new(handler),
            exceptions,
            reported: Mutex::default(),
            throw_on_report: AtomicBool::new(false),
        }
    }
    pub fn handler(&self) -> Arc<dyn ExceptionHandler> {
        self.handler.read().unwrap().clone()
    }
    pub fn set_handler(&self, handler: Arc<dyn ExceptionHandler>) -> &Self {
        let retired = std::mem::replace(&mut *self.handler.write().unwrap(), handler);
        drop(retired);
        self
    }
    pub fn reported(&self) -> Vec<SharedException> {
        self.reported.lock().unwrap().clone()
    }
    pub fn throw_on_report(&self) -> &Self {
        self.throw_on_report.store(true, Ordering::Release);
        self
    }
    pub fn throw_first_reported(&self) -> LogResult<()> {
        match self.reported().into_iter().next() {
            Some(error) => Err(LogError::WriteFailed(Box::new(error))),
            None => Ok(()),
        }
    }
    #[track_caller]
    pub fn assert_reported<E: Error + 'static>(&self) {
        self.assert_reported_where::<E>(|_| true);
    }
    #[track_caller]
    pub fn assert_reported_where<E: Error + 'static>(&self, predicate: impl Fn(&E) -> bool) {
        let found = self.reported().iter().any(|error| {
            root_error(error)
                .downcast_ref::<E>()
                .is_some_and(&predicate)
        });
        assert!(found, "未记录预期异常 {}", std::any::type_name::<E>());
    }
    #[track_caller]
    pub fn assert_not_reported<E: Error + 'static>(&self) {
        assert!(
            !self
                .reported()
                .iter()
                .any(|error| root_error(error).downcast_ref::<E>().is_some()),
            "记录了不应报告的异常 {}",
            std::any::type_name::<E>()
        );
    }
    #[track_caller]
    pub fn assert_not_reported_where<E: Error + 'static>(&self, predicate: impl Fn(&E) -> bool) {
        assert!(
            !self.reported().iter().any(|error| root_error(error)
                .downcast_ref::<E>()
                .is_some_and(&predicate)),
            "记录了匹配禁止条件的异常 {}",
            std::any::type_name::<E>()
        );
    }
    #[track_caller]
    pub fn assert_reported_count(&self, count: usize) {
        assert_eq!(self.reported().len(), count, "异常报告数量不符");
    }
    #[track_caller]
    pub fn assert_nothing_reported(&self) {
        self.assert_reported_count(0);
    }
    pub(crate) fn fakes(&self, exception: &Exception<'_>) -> bool {
        self.exceptions.is_empty() || self.exceptions.iter().any(|rule| rule.matches(exception))
    }
}
impl ExceptionHandler for ExceptionHandlerFake {
    fn register(&self) -> Result<(), BindingResolutionError> {
        self.handler().register()
    }
    fn build_context_for_exception(
        &self,
        exception: &Exception<'_>,
    ) -> Result<crate::log::LogContext, BindingResolutionError> {
        self.handler().build_context_for_exception(exception)
    }
    fn context_for_exception<'a, 'e>(
        &'a self,
        exception: &'a Exception<'e>,
    ) -> HttpFuture<'a, Result<crate::contracts::debug::ExceptionContext<'e>, BindingResolutionError>>
    {
        Box::pin(async move { self.handler().context_for_exception(exception).await })
    }
    fn context_for_exception_with_request<'a, 'e>(
        &'a self,
        request: &'a mut Request,
        exception: &'a Exception<'e>,
    ) -> HttpFuture<'a, Result<crate::contracts::debug::ExceptionContext<'e>, BindingResolutionError>>
    {
        Box::pin(async move {
            self.handler()
                .context_for_exception_with_request(request, exception)
                .await
        })
    }
    fn configure(
        &self,
        update: &mut dyn FnMut(&mut Exceptions),
    ) -> Result<(), BindingResolutionError> {
        self.handler().configure(update)
    }
    fn is_reporting(&self, exception: &Exception<'_>) -> Option<bool> {
        self.handler().is_reporting(exception)
    }
    fn should_stop_retries(&self, exception: &Exception<'_>) -> bool {
        self.handler().should_stop_retries(exception)
    }
    fn build_log_exception(
        &self,
        exception: &Exception<'_>,
        name: Option<&'static str>,
        reported: bool,
    ) -> serde_json::Value {
        self.handler()
            .build_log_exception(exception, name, reported)
    }
    fn should_report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async move { self.handler().should_report(exception).await })
    }
    fn report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async move {
            let handler = self.handler();
            if !self.fakes(exception) {
                return handler.report(exception).await;
            }
            if !handler.should_report(exception).await {
                return Ok(());
            }
            let error = exception
                .error()
                .downcast_ref::<SharedException>()
                .cloned()
                .ok_or_else(|| {
                    LogError::WriteFailed(Box::new(std::io::Error::other(
                        "记录异常需要共享所有权，请使用 Exceptions::report 或 SharedException",
                    )))
                })?;
            self.reported.lock().unwrap().push(error.clone());
            if self.throw_on_report.load(Ordering::Acquire) {
                return escape_report(error).await;
            }
            Ok(())
        })
    }
    fn report_with_request<'a>(
        &'a self,
        request: &'a mut Request,
        exception: &'a Exception<'_>,
    ) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async move {
            if self.fakes(exception) {
                self.report(exception).await
            } else {
                self.handler().report_with_request(request, exception).await
            }
        })
    }
    fn handle_http<'a>(
        &'a self,
        request: &'a mut Request,
        error: Arc<HttpError>,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move {
            let handler = self.handler();
            if !self.fakes(&Exception::Http(&error)) {
                return handler.handle_http(request, error).await;
            }
            let shared = SharedException::from_arc(error.clone());
            let _ = self.report(&Exception::Error(&shared)).await;
            handler.render_http_response(request, &error).await
        })
    }
    fn render_http_response<'a>(
        &'a self,
        request: &'a mut Request,
        error: &'a HttpError,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move { self.handler().render_http_response(request, error).await })
    }
    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response {
        self.handler().render(request, exception)
    }
    fn render_for_console(&self, output: &mut Output, exception: &Exception<'_>) {
        self.handler().render_for_console(output, exception);
    }
}
