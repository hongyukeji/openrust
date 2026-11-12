use std::{pin::Pin, time::Duration};

use futures_util::{Stream, stream};

use crate::{
    database::Transaction,
    queue::{QueueFuture, QueueResult},
};

use super::{FailedJob, PrunableFailedJobProvider};

/// Entries to retry in storage order. Each key identifies the stored record to remove.
pub type FailedJobRetryStream<'a> =
    Pin<Box<dyn Stream<Item = QueueResult<(String, FailedJob)>> + Send + 'a>>;

/// Stores and retrieves failed queue jobs.
pub trait FailedJobProviderInterface: Send + Sync {
    fn prunable(&self) -> Option<&dyn PrunableFailedJobProvider> {
        None
    }

    fn log<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>>;
    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>>;

    /// List the retry identifiers, optionally limited to the given queue.
    fn ids<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<Vec<String>>> {
        Box::pin(async move {
            Ok(self
                .all()
                .await?
                .into_iter()
                .filter(|job| queue.is_none_or(|queue| job.queue == queue))
                .map(|job| job.id)
                .collect())
        })
    }
    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>>;

    /// Resolve a retry identifier to one or more entries. An empty stream represents an existing
    /// selection without any records. The default adapts the provider's single-record lookup.
    #[allow(
        non_snake_case,
        reason = "Matches the failed job provider method naming"
    )]
    fn findForRetry<'a>(
        &'a self,
        id: &'a str,
    ) -> QueueFuture<'a, QueueResult<Option<FailedJobRetryStream<'a>>>> {
        Box::pin(async move {
            let Some(job) = self.find(id).await? else {
                return Ok(None);
            };
            let entries: FailedJobRetryStream<'a> =
                Box::pin(stream::iter([Ok((id.to_owned(), job))]));
            Ok(Some(entries))
        })
    }
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
