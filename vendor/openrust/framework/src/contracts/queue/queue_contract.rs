use std::time::Duration;

use crate::queue::{Delay, Payload, QueueFuture, QueueResult};

use super::{ClearableQueue, Job};

/// Counts observed together on a queue connection.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct QueueMetrics {
    pub size: u64,
    pub pending: Option<u64>,
    pub delayed: Option<u64>,
    pub reserved: Option<u64>,
    pub oldest_pending: Option<u64>,
}

/// Dispatches and reserves jobs on a queue connection.
#[allow(non_snake_case, reason = "Matches Laravel's queue monitoring API")]
pub trait Queue: Send + Sync {
    fn get_connection_name(&self) -> &str;

    fn clearable(&self) -> Option<&dyn ClearableQueue> {
        None
    }

    fn default_queue(&self) -> &str {
        "default"
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>>;

    fn metrics<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(async move {
            Ok(QueueMetrics {
                size: self.size(queue).await?,
                ..QueueMetrics::default()
            })
        })
    }

    fn pendingSize<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<u64>>> {
        Box::pin(async move { Ok(self.metrics(queue).await?.pending) })
    }

    fn delayedSize<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<u64>>> {
        Box::pin(async move { Ok(self.metrics(queue).await?.delayed) })
    }

    fn reservedSize<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<u64>>> {
        Box::pin(async move { Ok(self.metrics(queue).await?.reserved) })
    }

    fn creationTimeOfOldestPendingJob<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<u64>>> {
        Box::pin(async move { Ok(self.metrics(queue).await?.oldest_pending) })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>>;

    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>>;

    fn push_in_transaction<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
        _transaction: &'a mut crate::database::Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        match delay {
            Some(delay) => self.later(delay, payload, queue),
            None => self.push(payload, queue),
        }
    }

    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>>;
    /// Reserves an available job or waits up to the supplied limit on blocking drivers.
    fn pop_within<'a>(
        &'a self,
        queue: Option<&'a str>,
        _wait: Duration,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        self.pop_now(queue)
    }

    fn pop_now<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        self.pop(queue)
    }
}
