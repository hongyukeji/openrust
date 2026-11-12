use std::time::Duration;

use crate::{
    database::Transaction,
    queue::{QueueFuture, QueueResult},
};

use super::{FailedJob, PrunableFailedJobProvider};

/// Stores and retrieves failed queue jobs.
pub trait FailedJobProviderInterface: Send + Sync {
    fn prunable(&self) -> Option<&dyn PrunableFailedJobProvider> {
        None
    }

    fn log<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>>;
    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>>;
    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>>;
    fn forget<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<bool>>;
    fn forget_retry<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<bool>> {
        self.forget(&job.id)
    }
    /// Remove an unchanged failure inside the queue transaction if both stores share it.
    fn forget_retry_in_transaction<'a>(
        &'a self,
        _job: &'a FailedJob,
        _transaction: &'a mut Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<bool>>> {
        Box::pin(async { Ok(None) })
    }
    fn flush(&self, older_than: Option<Duration>) -> QueueFuture<'_, QueueResult<()>>;
}
