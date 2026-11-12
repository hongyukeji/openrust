use crate::queue::{JobContext, Payload, QueueFuture, QueueResult};

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
}
