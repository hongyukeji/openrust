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
}
impl Exceptions {
    pub async fn report(error: impl Error + Send + Sync + 'static) -> LogResult<()> {
        let shared = if let Some(shared) = (&error as &(dyn Error + 'static))
            .downcast_ref::<crate::contracts::debug::SharedException>(
        ) {
            shared.clone()
        } else {
            crate::contracts::debug::SharedException::new(error)
        };
        Self::get_facade_root()?
            .report(&Exception::Error(&shared))
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
    ) -> Result<Response, BindingResolutionError> {
        Ok(Self::get_facade_root()?.render(request, exception))
    }
    pub fn render_for_console(
        output: &mut Output,
        exception: &Exception<'_>,
    ) -> Result<(), BindingResolutionError> {
        Self::get_facade_root()?.render_for_console(output, exception);
        Ok(())
    }
    pub fn handler() -> Result<Arc<dyn ExceptionHandler>, BindingResolutionError> {
        Self::get_facade_root()
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
        callback: impl Fn(&Request, &crate::http::HttpError) -> bool + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::configure(|configuration| {
            configuration.should_render_json_when(callback);
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
