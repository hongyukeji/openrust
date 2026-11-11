use std::{sync::Arc, time::Duration};

use crate::contracts::queue::Job;

use super::super::{JobContext, JobOptions, Payload, Queue, QueueError, QueueFuture, QueueResult};

pub(in crate::queue) trait Reservation: Send {
    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>>;
    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>>;
}

pub(crate) struct ReservedJob {
    pub(in crate::queue) registry: Arc<Queue>,
    pub(in crate::queue) storage: Box<dyn Reservation>,
    pub(in crate::queue) id: String,
    pub(in crate::queue) context: JobContext<'static>,
    pub(in crate::queue) payload: Payload,
    pub(in crate::queue) deleted: bool,
    pub(in crate::queue) released: bool,
    pub(in crate::queue) failed: bool,
}

impl Job for ReservedJob {
    fn uuid(&self) -> &str {
        self.payload.uuid()
    }
    fn get_job_id(&self) -> &str {
        &self.id
    }
    fn get_connection_name(&self) -> &str {
        self.context.connection()
    }
    fn get_queue(&self) -> &str {
        self.context.queue()
    }
    fn payload(&self) -> &Payload {
        &self.payload
    }
    fn options(&self) -> &JobOptions {
        self.payload.options()
    }
    fn attempts(&self) -> u32 {
        self.context.attempts()
    }
    fn is_deleted(&self) -> bool {
        self.deleted
    }
    fn is_released(&self) -> bool {
        self.released
    }
    fn has_failed(&self) -> bool {
        self.failed
    }
    fn mark_as_failed(&mut self) {
        self.failed = true;
    }

    fn fire<'a>(&'a mut self, context: &'a mut JobContext<'_>) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            context.connection.clone_from(&self.context.connection);
            context.queue.clone_from(&self.context.queue);
            context.attempts = self.context.attempts;
            context.transaction = None;
            self.registry.fire(context, &self.payload).await
        })
    }

    fn fail<'a>(&'a mut self, _cause: Option<&'a QueueError>) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            self.delete().await?;
            self.failed = true;
            Ok(())
        })
    }

    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            if self.deleted || self.released {
                return Ok(());
            }
            self.storage.release(delay).await?;
            self.released = true;
            Ok(())
        })
    }

    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            if self.deleted || self.released {
                return Ok(());
            }
            self.storage.delete().await?;
            self.deleted = true;
            Ok(())
        })
    }
}
