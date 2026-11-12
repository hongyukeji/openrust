use std::sync::Arc;

use crate::contracts::queue::{Job, Queue as Contract};

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
            if next.is_none() {
                return Ok(None);
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
