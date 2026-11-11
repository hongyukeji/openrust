//! Rust adapter contract for a PSR-style log channel.

use crate::log::{Level, LogContext, LogResult};

/// Receives a structured log record.
pub trait Logger: Send + Sync {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()>;
}
