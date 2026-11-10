use crate::queue::{JobContext, Payload, QueueFuture, QueueResult};

/// Executes a registered application job.
pub trait JobHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>>;
}
