use chrono::{DateTime, Utc};

use crate::queue::{QueueFuture, QueueResult};

/// Removes failed jobs strictly older than a cutoff.
pub trait PrunableFailedJobProvider: Send + Sync {
    fn prune(&self, before: DateTime<Utc>) -> QueueFuture<'_, QueueResult<u64>>;
}
