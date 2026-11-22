use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{Level, LogContext, LogError, LogManager, LogResult, Logger, StackChannel};

/// Resolves the current application's log manager.
pub struct Log;

impl Facade for Log {
    type Service = LogManager;

    fn getFacadeAccessor() -> ServiceKey {
        "log".into()
    }
}

#[allow(non_snake_case, reason = "遵循 Laravel 日志原名")]
impl Log {
    pub fn withContext(context: LogContext) -> LogResult<Arc<Logger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .withContext(context)
    }
    pub fn withoutContext(keys: Option<&[&str]>) -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .withoutContext(keys);
        Ok(())
    }
    pub fn shareContext(context: LogContext) -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .shareContext(context);
        Ok(())
    }
    pub fn sharedContext() -> LogResult<LogContext> {
        Ok(Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .sharedContext())
    }
    pub fn flushSharedContext() -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .flushSharedContext();
        Ok(())
    }

    pub fn listen(
        listener: impl Fn(&super::events::MessageLogged) -> LogResult<()> + Send + Sync + 'static,
    ) -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .listen(listener)
    }

    pub fn build(config: &serde_json::Value) -> LogResult<Arc<Logger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .build(config)
    }

    pub fn getLogger() -> LogResult<Arc<super::NativeLogger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .getLogger()
    }

    pub fn getChannels() -> LogResult<std::collections::HashMap<String, Arc<Logger>>> {
        Ok(Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .getChannels())
    }

    pub fn channel(name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .channel(name)
    }

    pub fn stack(names: &[&str], name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .stack(names, name)
    }

    #[allow(non_snake_case, reason = "Typed adapter for Laravel's mixed log stack")]
    pub fn stackChannels(
        channels: &[StackChannel<'_>],
        name: Option<&str>,
    ) -> LogResult<Arc<Logger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .stackChannels(channels, name)
    }

    pub fn log(level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .log(level, message, context)
    }

    /// Writes a log at the current request boundary and awaits asynchronous observers.
    #[allow(non_snake_case, reason = "OpenRust asynchronous request log adapter")]
    pub async fn logWithRequest(
        request: &mut crate::http::Request,
        level: Level,
        message: &str,
        context: &LogContext,
    ) -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .logWithRequest(request, level, message, context)
            .await
    }

    #[allow(
        non_snake_case,
        reason = "Rust adapter for an exception in Laravel log context"
    )]
    pub fn logException<E: std::error::Error + Send + Sync + 'static>(
        level: Level,
        message: &str,
        error: &E,
        context: &LogContext,
    ) -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .logException(level, message, error, context)
    }

    log_methods!();
}
