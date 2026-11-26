use crate::queue::{QueueFuture, QueueResult};

/// Removes the available, delayed, and reserved jobs on a selected queue.
pub trait ClearableQueue: Send + Sync {
    fn clear<'a>(&'a self, queue: &'a str) -> QueueFuture<'a, QueueResult<u64>>;
}
