use crate::queue::QueueResult;

/// Handles an interruption while a queued job is running.
pub trait Interruptible: Send + Sync {
    fn interrupted(&self, signal: i32) -> QueueResult<()>;
}
