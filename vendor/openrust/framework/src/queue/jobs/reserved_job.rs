use std::{sync::Arc, time::Duration};

use crate::contracts::{events::Event, queue::Job};

use super::super::{
    Delay, JobContext, JobOptions, Payload, Queue, QueueError, QueueFuture, QueueResult,
    events::{JobSnapshot, RedisJobDeleted, RedisJobReleased},
};

pub(in crate::queue) trait Reservation: Send {
    fn advance_chain<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>>;
    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>>;
    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>>;
    fn redis_reserved_at(&self) -> Option<tokio::time::Instant> {
        None
    }
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

impl ReservedJob {
    async fn observe_redis_settlement<E: Event>(
        &mut self,
        name: &str,
        event: impl FnOnce(String, JobSnapshot, Duration, bool) -> E + Send,
    ) {
        let Some(reserved_at) = self.storage.redis_reserved_at() else {
            return;
        };
        if let Ok(manager) = self.registry.manager()
            && matches!(manager.observes_worker_event(name), Ok(true))
        {
            manager
                .observe_confirmed_event(event(
                    self.context.connection.clone(),
                    JobSnapshot::from_job(self),
                    reserved_at.elapsed(),
                    self.failed,
                ))
                .await;
        }
    }

    async fn observe_deletion(&mut self) {
        self.observe_redis_settlement(
            RedisJobDeleted::NAME,
            |connection_name, job, duration, failed| RedisJobDeleted {
                connection_name,
                job,
                failed,
                duration,
            },
        )
        .await;
    }
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

    fn supports_isolated_execution(&self) -> bool {
        true
    }

    fn fire<'a>(&'a mut self, context: &'a mut JobContext<'_>) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            context.connection.clone_from(&self.context.connection);
            context.queue.clone_from(&self.context.queue);
            context.attempts = self.context.attempts;
            context.transaction = None;
            context.release_after = None;
            self.registry.fire(context, &self.payload).await
        })
    }

    fn advance_chain<'a>(&'a mut self, next: &'a Payload) -> QueueFuture<'a, QueueResult<()>> {
        self.advance_chain_to(next, None, None)
    }

    fn advance_chain_to<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            if self.deleted || self.released {
                return Err(QueueError::LostReservation);
            }
            self.storage.advance_chain(next, queue, delay).await?;
            self.deleted = true;
            self.observe_deletion().await;
            Ok(())
        })
    }

    fn fail<'a>(&'a mut self, _cause: Option<&'a QueueError>) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let was_failed = self.failed;
            self.failed = true;
            if let Err(error) = self.delete().await {
                self.failed = was_failed;
                return Err(error);
            }
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
            self.observe_redis_settlement(RedisJobReleased::NAME, |connection_name, job, _, _| {
                RedisJobReleased {
                    connection_name,
                    job,
                    delay,
                }
            })
            .await;
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
            self.observe_deletion().await;
            Ok(())
        })
    }
}
