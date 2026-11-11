use std::time::Duration;

use crate::queue::{QueueFuture, QueueResult};

use super::FailedJob;

/// Stores and retrieves failed queue jobs.
pub trait FailedJobProviderInterface: Send + Sync {
    fn log<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>>;
    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>>;
    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>>;
    fn forget<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<bool>>;
    fn forget_retry<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<bool>> {
        self.forget(&job.id)
    }
    fn flush(&self, older_than: Option<Duration>) -> QueueFuture<'_, QueueResult<()>>;
}
