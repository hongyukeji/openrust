use crate::queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult};

use super::Interruptible;

/// Executes a registered application job.
pub trait JobHandler: Send + Sync {
    fn interruptible(&self) -> Option<&dyn Interruptible> {
        None
    }

    fn middleware(&self) -> Vec<std::sync::Arc<dyn crate::queue::middleware::JobMiddleware>> {
        Vec::new()
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>>;

    /// Runs after a job reaches terminal failure, with the original payload.
    fn failed<'a>(
        &'a self,
        _context: &'a mut JobContext<'_>,
        _payload: &'a Payload,
        _error: &'a QueueError,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
