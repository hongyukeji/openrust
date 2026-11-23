#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::sync::Arc;

use crate::{
    concurrency::{Task, enqueue_with_skip},
    contracts::queue::{Job, Queue as Contract, QueueMetrics},
};

use super::{Delay, Payload, Queue, QueueFuture, QueueResult, backend};

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
            let queue = super::queue_name(queue, &self.queue)?.to_owned();
            let manager = self.registry.manager()?;
            let skipped = payload.unique_lock.as_ref().map(|_| {
                let manager = manager.clone();
                let payload = payload.clone();
                Box::pin(async move { manager.release_lifecycle_lock(&payload).await }) as _
            });
            let id = payload.uuid().to_owned();
            let mut payload = payload.clone();
            let name = self.name.clone();
            let snapshot = manager.context_snapshot()?;
            match self.lifecycle {
                Lifecycle::Null => unreachable!(),
                Lifecycle::Deferred => {
                    let _ = enqueue_with_skip(
                        Box::pin(async move {
                            snapshot(&mut payload);
                            manager.run_deferred(name, queue, payload).await
                        }),
                        skipped,
                    )
                    .map_err(backend)?;
                }
                Lifecycle::Background => {
                    let driver = manager.background_driver()?;
                    let _ = enqueue_with_skip(
                        Box::pin(async move {
                            snapshot(&mut payload);
                            let launch = Task::new(BACKGROUND_TASK, (name, queue, &payload))
                                .and_then(|task| driver.launch_detached(task, &payload.context));
                            if let Err(error) = launch {
                                manager.release_lifecycle_lock(&payload).await?;
                                return Err(backend(error));
                            }
                            Ok(())
                        }),
                        skipped,
                    )
                    .map_err(backend)?;
                }
            }
            Ok(Some(id))
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
