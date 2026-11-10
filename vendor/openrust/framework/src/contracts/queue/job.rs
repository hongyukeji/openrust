use std::time::Duration;

use crate::queue::{JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueResult};

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
    fn fire<'a>(&'a mut self, context: &'a mut JobContext) -> QueueFuture<'a, QueueResult<()>>;
    fn fail<'a>(&'a mut self, cause: Option<&'a QueueError>) -> QueueFuture<'a, QueueResult<()>>;
    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>>;
    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>>;
}
