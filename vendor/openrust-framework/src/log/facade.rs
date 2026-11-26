use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{Level, LogContext, LogManager, LogResult, Logger, StackChannel};

/// Resolves the current application's log manager.
pub struct Log;

impl Facade for Log {
    type Service = LogManager;

    fn get_facade_accessor() -> ServiceKey {
        "log".into()
    }
}

impl Log {
    pub fn driver(name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.driver(name)
    }
    pub fn get_default_driver() -> LogResult<Option<String>> {
        Ok(Self::get_facade_root()?.get_default_driver())
    }
    pub fn set_default_driver(name: &str) -> Result<(), crate::config::ConfigError> {
        Self::get_facade_root()?.set_default_driver(name)
    }
    pub fn forget_channel(name: Option<&str>) -> LogResult<()> {
        Self::get_facade_root()?.forget_channel(name);
        Ok(())
    }
    pub fn extend(
        driver: impl Into<String>,
        factory: impl Fn(
            &LogManager,
            &serde_json::Value,
        ) -> LogResult<Arc<dyn crate::contracts::log::Logger>>
        + Send
        + Sync
        + 'static,
    ) -> LogResult<()> {
        Self::get_facade_root()?.extend(driver, factory);
        Ok(())
    }
    pub fn with_context(context: LogContext) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.with_context(context)
    }
    pub fn without_context(keys: Option<&[&str]>) -> LogResult<()> {
        Self::get_facade_root()?.without_context(keys);
        Ok(())
    }
    pub fn share_context(context: LogContext) -> LogResult<()> {
        Self::get_facade_root()?.share_context(context);
        Ok(())
    }
    pub fn shared_context() -> LogResult<LogContext> {
        Ok(Self::get_facade_root()?.shared_context())
    }
    pub fn flush_shared_context() -> LogResult<()> {
        Self::get_facade_root()?.flush_shared_context();
        Ok(())
    }

    pub fn listen(
        listener: impl Fn(&super::events::MessageLogged) -> LogResult<()> + Send + Sync + 'static,
    ) -> LogResult<()> {
        Self::get_facade_root()?.listen(listener)
    }

    pub fn build(config: &serde_json::Value) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.build(config)
    }

    pub fn get_logger() -> LogResult<Arc<super::NativeLogger>> {
        Self::get_facade_root()?.get_logger()
    }

    pub fn get_channels() -> LogResult<std::collections::HashMap<String, Arc<Logger>>> {
        Ok(Self::get_facade_root()?.get_channels())
    }

    pub fn channel(name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.channel(name)
    }

    pub fn stack(names: &[&str], name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.stack(names, name)
    }

    pub fn stack_channels(
        channels: &[StackChannel<'_>],
        name: Option<&str>,
    ) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.stack_channels(channels, name)
    }

    pub fn log(level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::get_facade_root()?.log(level, message, context)
    }

    /// Writes a log at the current request boundary and awaits asynchronous observers.
    pub async fn log_with_request(
        request: &mut crate::http::Request,
        level: Level,
        message: &str,
        context: &LogContext,
    ) -> LogResult<()> {
        Self::get_facade_root()?
            .log_with_request(request, level, message, context)
            .await
    }

    pub fn log_exception<E: std::error::Error + Send + Sync + 'static>(
        level: Level,
        message: &str,
        error: &E,
        context: &LogContext,
    ) -> LogResult<()> {
        Self::get_facade_root()?.log_exception(level, message, error, context)
    }

    log_methods!();
}
