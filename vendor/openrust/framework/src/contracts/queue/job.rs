use std::time::Duration;

use crate::queue::{Delay, JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueResult};

/// A single reservation of a queued job.
pub trait Job: Send {
    fn uuid(&self) -> &str;
    fn get_job_id(&self) -> &str;
    fn get_connection_name(&self) -> &str;
    fn get_queue(&self) -> &str;
    fn payload(&self) -> &Payload;
    fn options(&self) -> &JobOptions;
    fn attempts(&self) -> u32;
    fn is_deleted(&self) -> bool;
    fn is_released(&self) -> bool;
    fn has_failed(&self) -> bool;
    fn mark_as_failed(&mut self);
    /// Whether the payload can be executed by the application registry in another process.
    fn supports_isolated_execution(&self) -> bool {
        false
    }
    fn fire<'a>(&'a mut self, context: &'a mut JobContext<'_>) -> QueueFuture<'a, QueueResult<()>>;
    fn fail<'a>(&'a mut self, cause: Option<&'a QueueError>) -> QueueFuture<'a, QueueResult<()>>;
    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>>;
    /// Atomically acknowledges this reservation and enqueues its successor.
    fn advance_chain<'a>(&'a mut self, _next: &'a Payload) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async { Err(QueueError::InvalidConfiguration) })
    }
    /// Acknowledges this reservation and enqueues its successor on the same connection.
    fn advance_chain_to<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        if queue.is_none() && delay.is_none() {
            return self.advance_chain(next);
        }
        Box::pin(async { Err(QueueError::InvalidConfiguration) })
    }
    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>>;
}
