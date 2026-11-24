use crate::{
    console::Output,
    container::ServiceKey,
    contracts::{
        container::BindingResolutionError,
        debug::{Exception, ExceptionHandler},
    },
    http::{Request, Response},
    log::LogResult,
    support::facades::Facade,
};
use std::{error::Error, sync::Arc};

/// 当前应用实际异常处理契约，HTTP、Artisan 与 Worker 复用同一绑定。
pub struct Exceptions;
impl Facade for Exceptions {
    type Service = dyn ExceptionHandler;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<dyn ExceptionHandler>()
    }
    fn is_fake_root(service: &Self::Service) -> bool {
        (service as &dyn std::any::Any).is::<crate::testing::ExceptionHandlerFake>()
    }
}
impl Exceptions {
    /// 捕获替身的重抛信号，以 Result 返回原始错误并中止本次异步调用。
    pub async fn catch_reported<T>(
        future: impl std::future::Future<Output = T>,
    ) -> Result<T, crate::contracts::debug::SharedException> {
        crate::testing::exception_handler_fake::catch_reported(future).await
    }
    pub fn map<E, M>(
        callback: impl Fn(&E) -> M + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError>
    where
        E: Error + 'static,
        M: Error + Send + Sync + 'static,
    {
        Self::configure(|configuration| {
            configuration.map(callback);
        })
    }
    pub fn map_optional<E, M>(
        callback: impl Fn(&E) -> Option<M> + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError>
    where
        E: Error + 'static,
        M: Error + Send + Sync + 'static,
    {
        Self::configure(|configuration| {
            configuration.map_optional(callback);
        })
    }
    pub fn inner_exception<E: Error + 'static>(
        callback: impl Fn(&E) -> Option<crate::contracts::debug::SharedException>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.inner_exception(callback);
        })
    }
    pub fn register() -> Result<(), BindingResolutionError> {
        Self::get_facade_root()?.register()
    }
    pub fn build_context_for_exception(
        exception: &Exception<'_>,
    ) -> Result<crate::log::LogContext, BindingResolutionError> {
        Self::get_facade_root()?.build_context_for_exception(exception)
    }
    pub async fn context_for_exception<'e>(
        exception: &Exception<'e>,
    ) -> Result<crate::contracts::debug::ExceptionContext<'e>, BindingResolutionError> {
        Self::get_facade_root()?
            .context_for_exception(exception)
            .await
    }
    pub async fn context_for_exception_with_request<'e>(
        request: &mut Request,
        exception: &Exception<'e>,
    ) -> Result<crate::contracts::debug::ExceptionContext<'e>, BindingResolutionError> {
        Self::get_facade_root()?
            .context_for_exception_with_request(request, exception)
            .await
    }
    pub fn exception_context<E: Error + 'static>(
        callback: impl Fn(&E) -> crate::log::LogContext + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.exception_context(callback);
        })
    }
    pub fn build_context_using_with_context(
        callback: impl Fn(&Exception<'_>, &crate::log::LogContext) -> crate::log::LogContext
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.context_with(callback);
        })
    }
    pub async fn report(error: impl Error + Send + Sync + 'static) -> LogResult<()> {
        let shared = crate::contracts::debug::SharedException::new(error);
        Self::get_facade_root()?
            .report(&Exception::Error(&shared))
            .await
    }
    pub async fn report_for_request(
        request: &mut Request,
        error: impl Error + Send + Sync + 'static,
    ) -> LogResult<()> {
        let shared = crate::contracts::debug::SharedException::new(error);
        Self::get_facade_root()?
            .report_with_request(request, &Exception::Error(&shared))
            .await
    }
    pub async fn should_report(exception: &Exception<'_>) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.should_report(exception).await)
    }
    pub fn should_stop_retries(exception: &Exception<'_>) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.should_stop_retries(exception))
    }
    pub fn render(
        request: &Request,
        exception: &Exception<'_>,
    ) -> Result<Response, Box<dyn Error + Send + Sync>> {
        Ok(Self::get_facade_root()?.render_result(request, exception)?)
    }
    pub fn render_for_console(
        output: &mut Output,
        exception: &Exception<'_>,
    ) -> Result<(), BindingResolutionError> {
        Self::get_facade_root()?.render_for_console(output, exception);
        Ok(())
    }
    pub fn handler() -> Result<Arc<dyn ExceptionHandler>, BindingResolutionError> {
        let current = Self::get_facade_root()?;
        let any: Arc<dyn std::any::Any + Send + Sync> = current.clone();
        Ok(
            match any.downcast::<crate::testing::ExceptionHandlerFake>() {
                Ok(fake) => fake.handler(),
                Err(_) => current,
            },
        )
    }
}

impl Exceptions {
    fn configure<T>(
        update: impl FnOnce(&mut crate::foundation::configuration::Exceptions) -> T,
    ) -> Result<T, BindingResolutionError> {
        let mut update = Some(update);
        let mut result = None;
        Self::get_facade_root()?.configure(&mut |configuration| {
            if let Some(update) = update.take() {
                result = Some(update(configuration));
            }
        })?;
        result.ok_or_else(|| BindingResolutionError::FactoryFailed {
            service: "ExceptionHandler",
            source: Box::new(std::io::Error::other("处理器未执行配置注册")),
        })
    }
    pub fn is_reporting(exception: &Exception<'_>) -> Result<bool, BindingResolutionError> {
        Self::get_facade_root()?
            .is_reporting(exception)
            .ok_or_else(|| BindingResolutionError::FactoryFailed {
                service: "ExceptionHandler",
                source: Box::new(std::io::Error::other(
                    "此异常处理器未公开正在报告的异常查询",
                )),
            })
    }
    pub fn reportable<E: Error + 'static>(
        callback: impl Fn(&E) -> crate::foundation::configuration::ReportDecision
        + Send
        + Sync
        + 'static,
    ) -> Result<super::ReportableHandler, BindingResolutionError> {
        Self::configure(|configuration| configuration.reportable(callback))
    }
    pub fn reportable_all(
        callback: impl Fn(&Exception<'_>) -> crate::foundation::configuration::ReportDecision
        + Send
        + Sync
        + 'static,
    ) -> Result<super::ReportableHandler, BindingResolutionError> {
        Self::configure(|configuration| configuration.reportable_all(callback))
    }
    pub fn renderable<E: Error + 'static>(
        callback: impl Fn(&E, &Request) -> Option<Response> + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.render(callback);
        })
    }
    /// 为当前应用注册异常类型自身的报告、渲染和上下文方法。
    pub fn use_exception<E: crate::contracts::debug::ExceptionBehavior>()
    -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.use_exception::<E>();
        })
    }

    pub fn dont_report<E: Error + 'static>() -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.dont_report::<E>();
        })
    }
    pub fn ignore<E: Error + 'static>() -> Result<(), BindingResolutionError> {
        Self::dont_report::<E>()
    }
    pub fn stop_ignoring<E: Error + 'static>() -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.stop_ignoring::<E>();
        })
    }
    pub fn dont_report_when(
        callback: impl Fn(&Exception<'_>) -> bool + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.dont_report_when(callback);
        })
    }
    pub fn dont_retry<E: Error + 'static>() -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.dont_retry::<E>();
        })
    }
    pub fn dont_retry_when(
        callback: impl Fn(&Exception<'_>) -> bool + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.dont_retry_when(callback);
        })
    }
    pub fn dont_report_duplicates() -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.dont_report_duplicates();
        })
    }
    pub fn dont_flash(
        fields: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.dont_flash(fields);
        })
    }
    pub fn level<E: Error + 'static>(
        level: crate::log::Level,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.level::<E>(level);
        })
    }
    pub fn build_context_using(
        callback: impl Fn(&Exception<'_>) -> crate::log::LogContext + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.context(callback);
        })
    }
    pub fn respond_using(
        callback: impl Fn(Response, &Exception<'_>, &Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.respond(callback);
        })
    }
    pub fn should_render_json_when(
        callback: impl Fn(&Request, &Exception<'_>) -> bool + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.should_render_json_when(callback);
        })
    }
    /// 为全部错误注册策略，复用与具类型策略相同的有序回调集合。
    pub fn throttle_using_all(
        callback: impl Fn(&Exception<'_>) -> Option<crate::foundation::configuration::ReportThrottle>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.throttle_all(callback);
        })
    }
    pub fn throttle_using<E: Error + 'static>(
        callback: impl Fn(&E) -> Option<crate::foundation::configuration::ReportThrottle>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.throttle(callback);
        })
    }
}

impl Exceptions {
    fn fake_root() -> Result<Arc<crate::testing::ExceptionHandlerFake>, BindingResolutionError> {
        let current: Arc<dyn std::any::Any + Send + Sync> = Self::get_facade_root()?;
        current
            .downcast()
            .map_err(|_| BindingResolutionError::FactoryFailed {
                service: "ExceptionHandlerFake",
                source: Box::new(std::io::Error::other("尚未启用异常替身")),
            })
    }
    pub fn fake() -> Result<Arc<crate::testing::ExceptionHandlerFake>, BindingResolutionError> {
        Self::fake_with(Vec::new())
    }
    pub fn fake_with(
        exceptions: Vec<crate::testing::ExceptionType>,
    ) -> Result<Arc<crate::testing::ExceptionHandlerFake>, BindingResolutionError> {
        let fake = Arc::new(crate::testing::ExceptionHandlerFake::new(
            Self::handler()?,
            exceptions,
        ));
        Self::swap(fake.clone())?;
        Ok(fake)
    }
    pub fn reported()
    -> Result<Vec<crate::contracts::debug::SharedException>, BindingResolutionError> {
        Ok(Self::fake_root()?.reported())
    }
    pub fn set_handler(
        handler: Arc<dyn ExceptionHandler>,
    ) -> Result<Arc<crate::testing::ExceptionHandlerFake>, BindingResolutionError> {
        let fake = Self::fake_root()?;
        fake.set_handler(handler);
        Ok(fake)
    }
    pub fn throw_on_report()
    -> Result<Arc<crate::testing::ExceptionHandlerFake>, BindingResolutionError> {
        let fake = Self::fake_root()?;
        fake.throw_on_report();
        Ok(fake)
    }
    pub fn throw_first_reported() -> crate::log::LogResult<Arc<crate::testing::ExceptionHandlerFake>>
    {
        let fake = Self::fake_root()?;
        fake.throw_first_reported()?;
        Ok(fake)
    }
    #[track_caller]
    pub fn assert_reported<E: Error + 'static>() -> Result<(), BindingResolutionError> {
        Self::fake_root()?.assert_reported::<E>();
        Ok(())
    }
    #[track_caller]
    pub fn assert_reported_where<E: Error + 'static>(
        predicate: impl Fn(&E) -> bool,
    ) -> Result<(), BindingResolutionError> {
        Self::fake_root()?.assert_reported_where(predicate);
        Ok(())
    }
    #[track_caller]
    pub fn assert_not_reported<E: Error + 'static>() -> Result<(), BindingResolutionError> {
        Self::fake_root()?.assert_not_reported::<E>();
        Ok(())
    }
    #[track_caller]
    pub fn assert_not_reported_where<E: Error + 'static>(
        predicate: impl Fn(&E) -> bool,
    ) -> Result<(), BindingResolutionError> {
        Self::fake_root()?.assert_not_reported_where(predicate);
        Ok(())
    }
    #[track_caller]
    pub fn assert_reported_count(count: usize) -> Result<(), BindingResolutionError> {
        Self::fake_root()?.assert_reported_count(count);
        Ok(())
    }
    #[track_caller]
    pub fn assert_nothing_reported() -> Result<(), BindingResolutionError> {
        Self::fake_root()?.assert_nothing_reported();
        Ok(())
    }
}
