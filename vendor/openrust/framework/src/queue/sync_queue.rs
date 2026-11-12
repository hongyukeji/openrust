use std::sync::Arc;

use crate::contracts::queue::{Job, Queue as Contract, QueueMetrics};

use super::{Delay, JobContext, Payload, Queue, QueueFuture, QueueResult, queue_name};

/// Executes dispatched jobs in the current process.
pub struct SyncQueue {
    pub(super) name: String,
    pub(super) registry: Arc<Queue>,
}

impl SyncQueue {
    async fn execute(
        &self,
        payload: &Payload,
        queue: Option<&str>,
        transaction: Option<&mut crate::database::Transaction>,
    ) -> QueueResult<Option<String>> {
        let queue = queue_name(queue, "default")?;
        let mut context = JobContext {
            connection: self.name.clone(),
            queue: queue.into(),
            attempts: 1,
            transaction,
            ..JobContext::default()
        };
        let mut next = None;
        loop {
            let job = next.as_ref().unwrap_or(payload);
            if let Err(error) = self.registry.fire(&mut context, job).await {
                if !job.chain_catches().is_empty() {
                    self.registry.run_chain_catches(job, &error).await?;
                }
                return Err(error);
            }
            if context.release_after.is_some() {
                return Ok(None);
            }
            next = context
                .chain_update
                .take()
                .map_or_else(|| job.next_chain_job(), |update| Some(update.next));
            let Some(successor) = &next else {
                return Ok(None);
            };
            if successor.chain_route.is_some() || successor.dispatch_options.is_some() {
                let mut options = successor.chain_options(&self.name, &context.queue);
                let manager = self.registry.manager()?;
                let connection = manager.connection(options.connection.as_deref())?;
                if connection.get_connection_name() != self.name {
                    options.connection = Some(connection.get_connection_name().to_owned());
                    let successor = next.take().expect("successor is present");
                    manager
                        .dispatch(successor, options, context.transaction())
                        .await?;
                    return Ok(None);
                }
                context.queue = queue_name(options.queue.as_deref(), "default")?.to_owned();
            }
        }
    }
}

impl Contract for SyncQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }

    fn size<'a>(&'a self, _queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async { Ok(0) })
    }

    fn metrics<'a>(
        &'a self,
        _queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(async {
            Ok(QueueMetrics {
                size: 0,
                pending: Some(0),
                delayed: Some(0),
                reserved: Some(0),
                oldest_pending: None,
            })
        })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.execute(payload, queue, None))
    }

    fn push_in_transaction<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
        _delay: Option<Delay>,
        transaction: &'a mut crate::database::Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.execute(payload, queue, Some(transaction)))
    }

    fn later<'a>(
        &'a self,
        _delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        self.push(payload, queue)
    }

    fn pop<'a>(
        &'a self,
        _queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(async { Ok(None) })
    }
}
