use std::sync::Arc;

use crate::{
    cache::Ttl,
    container::ServiceKey,
    contracts::queue::{JobHandler, Queue},
    database::Transaction,
    support::facades::Facade,
};

use super::{
    DispatchOptions, DispatchResult, JobPopCallback, Payload, QueueError, QueueManager, QueueResult,
};

/// Resolves the current application's queue manager.
pub struct QueueFacade;

impl Facade for QueueFacade {
    type Service = QueueManager;

    fn getFacadeAccessor() -> ServiceKey {
        "queue".into()
    }
}

impl QueueFacade {
    pub fn manager() -> QueueResult<Arc<QueueManager>> {
        Self::getFacadeRoot().map_err(QueueError::Unavailable)
    }

    pub fn register(name: &str, handler: Arc<dyn JobHandler>) -> QueueResult<()> {
        Self::manager()?.register(name, handler)
    }

    /// Sets or removes a reservation callback for a named worker in the current application.
    #[allow(non_snake_case, reason = "Matches Laravel Worker::popUsing")]
    pub fn popUsing(worker: &str, callback: Option<JobPopCallback>) -> QueueResult<()> {
        Self::manager()?.popUsing(worker, callback);
        Ok(())
    }

    pub fn connection(name: Option<&str>) -> QueueResult<Arc<dyn Queue>> {
        Self::manager()?.connection(name)
    }

    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub fn withoutInterruptionPolling() -> QueueResult<()> {
        Self::manager()?.withoutInterruptionPolling();
        Ok(())
    }

    pub async fn pause(connection: &str, queue: &str) -> QueueResult<()> {
        Self::manager()?.pause(connection, queue).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub async fn pauseFor(connection: &str, queue: &str, ttl: Ttl) -> QueueResult<()> {
        Self::manager()?.pauseFor(connection, queue, ttl).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub async fn pauseAll() -> QueueResult<()> {
        Self::manager()?.pauseAll().await
    }

    pub async fn resume(connection: &str, queue: &str) -> QueueResult<()> {
        Self::manager()?.resume(connection, queue).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub async fn resumeAll() -> QueueResult<()> {
        Self::manager()?.resumeAll().await
    }

    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub async fn isPaused(connection: &str, queue: &str) -> QueueResult<bool> {
        Self::manager()?.isPaused(connection, queue).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub async fn getPausedQueues<'a>(
        connection: &str,
        queues: &[&'a str],
    ) -> QueueResult<Vec<&'a str>> {
        Self::manager()?.getPausedQueues(connection, queues).await
    }

    pub async fn dispatch(
        payload: Payload,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<DispatchResult> {
        Self::manager()?
            .dispatch(payload, options, transaction)
            .await
    }
}
