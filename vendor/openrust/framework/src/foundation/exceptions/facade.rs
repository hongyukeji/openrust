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
        Self::get_facade_root()?
            .report(&Exception::Error(&error))
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
