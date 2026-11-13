use std::sync::Arc;

use crate::{
    concurrency::{Concurrency, Task, enqueue_deferred},
    contracts::queue::{Job, Queue as Contract, QueueMetrics},
};

use super::{Delay, Payload, Queue, QueueFuture, QueueResult, SyncQueue, backend};

pub(super) const BACKGROUND_TASK: &str = "openrust.queue.background";

#[derive(Clone, Copy)]
pub(super) enum Lifecycle {
    Null,
    Deferred,
    Background,
}

/// A queue that does not store reservations.
pub(super) struct LifecycleQueue {
    pub(super) name: String,
    pub(super) queue: String,
    pub(super) registry: Arc<Queue>,
    pub(super) lifecycle: Lifecycle,
}

impl Contract for LifecycleQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }

    fn default_queue(&self) -> &str {
        &self.queue
    }

    fn size<'a>(&'a self, _: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async { Ok(0) })
    }

    fn metrics<'a>(&'a self, _: Option<&'a str>) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(async {
            Ok(QueueMetrics {
                pending: Some(0),
                delayed: Some(0),
                reserved: Some(0),
                ..Default::default()
            })
        })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(async move {
            if matches!(self.lifecycle, Lifecycle::Null) {
                return Ok(None);
            }
            if payload.unique_lock.is_some() {
                return Err(super::QueueError::InvalidConfiguration);
            }
            let queue = super::queue_name(queue, &self.queue)?.to_owned();
            match self.lifecycle {
                Lifecycle::Null => unreachable!(),
                Lifecycle::Deferred => {
                    let sync = SyncQueue {
                        name: self.name.clone(),
                        registry: self.registry.clone(),
                    };
                    let payload = payload.clone();
                    let _ = enqueue_deferred(Box::pin(async move {
                        sync.push(&payload, Some(&queue)).await.map(|_| ())
                    }))
                    .map_err(backend)?;
                }
                Lifecycle::Background => {
                    let task = Task::new(BACKGROUND_TASK, (&self.name, queue, payload))
                        .map_err(backend)?;
                    let _ = Concurrency::driver(Some("process"))
                        .map_err(backend)?
                        .defer_detached(task)
                        .map_err(backend)?;
                }
            }
            Ok(None)
        })
    }

    fn later<'a>(
        &'a self,
        _: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        self.push(payload, queue)
    }

    fn pop<'a>(&'a self, _: Option<&'a str>) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(async { Ok(None) })
    }
}
