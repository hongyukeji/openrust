use std::time::Duration;

use crate::queue::{Delay, Payload, QueueFuture, QueueResult};

use super::{ClearableQueue, Job};

/// Dispatches and reserves jobs on a queue connection.
pub trait Queue: Send + Sync {
    fn get_connection_name(&self) -> &str;

    fn clearable(&self) -> Option<&dyn ClearableQueue> {
        None
    }

    fn default_queue(&self) -> &str {
        "default"
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>>;

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
