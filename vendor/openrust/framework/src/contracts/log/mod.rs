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
}
