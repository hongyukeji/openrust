//! Rust adapter contract for a PSR-style log channel.

use crate::log::{Level, LogContext, LogResult};

/// Receives a structured log record.
pub trait Logger: Send + Sync {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()>;

    /// Receives the record fields and application metadata separately.
    #[allow(non_snake_case, reason = "OpenRust structured log adapter")]
    fn logWithContext(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        _extra: &LogContext,
    ) -> LogResult<()> {
        self.log(level, message, context)
    }

    /// Receives a normalized exception separately from the ordinary context.
    #[allow(non_snake_case, reason = "OpenRust structured log adapter")]
    fn logWithException(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: Option<&LogContext>,
        _exception: &serde_json::Value,
    ) -> LogResult<()> {
        match extra {
            Some(extra) => self.logWithContext(level, message, context, extra),
            None => self.log(level, message, context),
        }
    }
}
