use crate::log::{Level, LogContext};

/// The record emitted after the logger accepts a message.
#[derive(Clone, Debug)]
pub struct MessageLogged {
    pub level: Level,
    pub message: String,
    pub context: LogContext,
    /// The native error snapshot, distinct from user-supplied JSON context.
    pub exception: Option<serde_json::Value>,
}

impl MessageLogged {
    pub const NAME: &'static str = r"OpenRust\Log\Events\MessageLogged";
}

impl crate::contracts::events::Event for MessageLogged {
    fn name(&self) -> &str {
        Self::NAME
    }
}
