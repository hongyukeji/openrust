use jiff::Timestamp;

use crate::queue::{QueueFuture, QueueResult};

/// Removes failed jobs strictly older than a cutoff.
pub trait PrunableFailedJobProvider: Send + Sync {
    fn prune(&self, before: Timestamp) -> QueueFuture<'_, QueueResult<u64>>;
}
