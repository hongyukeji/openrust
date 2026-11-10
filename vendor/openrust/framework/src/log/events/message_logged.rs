use crate::log::{Level, LogContext};

/// The record emitted after the logger accepts a message.
pub struct MessageLogged {
    pub level: Level,
    pub message: String,
    pub context: LogContext,
}
