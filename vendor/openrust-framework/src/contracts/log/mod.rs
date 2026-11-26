//! Rust adapter contract for a PSR-style log channel.

use crate::log::{Level, LogContext, LogResult};

/// Receives a structured log record.
pub trait Logger: Send + Sync {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()>;

    /// Receives the record fields and application metadata separately.
    fn log_with_context(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        _extra: &LogContext,
    ) -> LogResult<()> {
        self.log(level, message, context)
    }

    /// Receives a normalized exception separately from the ordinary context.
    fn log_with_exception(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: Option<&LogContext>,
        _exception: &serde_json::Value,
    ) -> LogResult<()> {
        match extra {
            Some(extra) => self.log_with_context(level, message, context, extra),
            None => self.log(level, message, context),
        }
    }
    /// Request-aware adapter; custom synchronous loggers retain their existing contract.
    fn log_with_exception_for_request<'a>(
        &'a self,
        _request: &'a mut crate::http::Request,
        level: Level,
        message: &'a str,
        context: &'a LogContext,
        exception: &'a serde_json::Value,
    ) -> crate::http::HttpFuture<'a, LogResult<()>> {
        Box::pin(async move { self.log_with_exception(level, message, context, None, exception) })
    }
}
