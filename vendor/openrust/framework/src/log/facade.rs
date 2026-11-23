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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Log {
    pub fn withContext(context: LogContext) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.withContext(context)
    }
    pub fn withoutContext(keys: Option<&[&str]>) -> LogResult<()> {
        Self::get_facade_root()?.withoutContext(keys);
        Ok(())
    }
    pub fn shareContext(context: LogContext) -> LogResult<()> {
        Self::get_facade_root()?.shareContext(context);
        Ok(())
    }
    pub fn sharedContext() -> LogResult<LogContext> {
        Ok(Self::get_facade_root()?.sharedContext())
    }
    pub fn flushSharedContext() -> LogResult<()> {
        Self::get_facade_root()?.flushSharedContext();
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

    pub fn getLogger() -> LogResult<Arc<super::NativeLogger>> {
        Self::get_facade_root()?.getLogger()
    }

    pub fn getChannels() -> LogResult<std::collections::HashMap<String, Arc<Logger>>> {
        Ok(Self::get_facade_root()?.getChannels())
    }

    pub fn channel(name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.channel(name)
    }

    pub fn stack(names: &[&str], name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.stack(names, name)
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn stackChannels(
        channels: &[StackChannel<'_>],
        name: Option<&str>,
    ) -> LogResult<Arc<Logger>> {
        Self::get_facade_root()?.stackChannels(channels, name)
    }

    pub fn log(level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::get_facade_root()?.log(level, message, context)
    }

    /// Writes a log at the current request boundary and awaits asynchronous observers.
    #[allow(non_snake_case, reason = "异步请求日志适配接口")]
    pub async fn logWithRequest(
        request: &mut crate::http::Request,
        level: Level,
        message: &str,
        context: &LogContext,
    ) -> LogResult<()> {
        Self::get_facade_root()?
            .logWithRequest(request, level, message, context)
            .await
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn logException<E: std::error::Error + Send + Sync + 'static>(
        level: Level,
        message: &str,
        error: &E,
        context: &LogContext,
    ) -> LogResult<()> {
        Self::get_facade_root()?.logException(level, message, error, context)
    }

    log_methods!();
}
