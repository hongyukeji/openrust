use std::sync::Arc;

use tokio::time::Instant;

use crate::contracts::queue::{Job, Queue};

use super::{QueueFuture, QueueResult};

/// Selects a reservation for a named worker, using its configured connection.
pub type JobPopCallback = Arc<
    dyn for<'a> Fn(JobPopper<'a>, &'a str) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>>
        + Send
        + Sync,
>;

/// Reserves a job using the current worker's polling limits.
pub struct JobPopper<'a> {
    connection: &'a dyn Queue,
    immediate: bool,
    deadline: Option<Instant>,
}

impl<'a> JobPopper<'a> {
    pub(super) fn new(
        connection: &'a dyn Queue,
        immediate: bool,
        deadline: Option<Instant>,
    ) -> Self {
        Self {
            connection,
            immediate,
            deadline,
        }
    }

    /// Reserves a job on the given queue, or on the connection's default queue.
    pub async fn pop(&self, queue: Option<&str>) -> QueueResult<Option<Box<dyn Job>>> {
        if self.immediate {
            self.connection.pop_now(queue).await
        } else if let Some(deadline) = self.deadline {
            let wait = deadline.saturating_duration_since(Instant::now());
            self.connection.pop_within(queue, wait).await
        } else {
            self.connection.pop(queue).await
        }
    }

    /// Checks a queue for an available job without waiting.
    pub async fn pop_now(&self, queue: Option<&str>) -> QueueResult<Option<Box<dyn Job>>> {
        self.connection.pop_now(queue).await
    }
}
