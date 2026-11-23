use std::sync::Arc;

use serde_json::Value;

use crate::{
    cache::Ttl,
    container::{Container, ServiceKey},
    contracts::queue::{JobHandler, Queue},
    database::Transaction,
    support::facades::Facade,
};

use super::{DispatchOptions, DispatchResult, JobPopCallback, Payload, QueueManager, QueueResult};

/// Resolves the current application's queue manager.
pub struct QueueFacade;

impl Facade for QueueFacade {
    type Service = QueueManager;

    fn get_facade_accessor() -> ServiceKey {
        "queue".into()
    }
}

impl QueueFacade {
    pub fn manager() -> QueueResult<Arc<QueueManager>> {
        Self::get_facade_root().map_err(Into::into)
    }

    pub fn register(name: &str, handler: Arc<dyn JobHandler>) -> QueueResult<()> {
        Self::manager()?.register(name, handler)
    }

    /// Sets or removes a reservation callback for a named worker in the current application.
    pub fn pop_using(worker: &str, callback: Option<JobPopCallback>) -> QueueResult<()> {
        Self::manager()?.pop_using(worker, callback);
        Ok(())
    }

    pub fn connection(name: Option<&str>) -> QueueResult<Arc<dyn Queue>> {
        Self::manager()?.connection(name)
    }

    /// Registers a connection driver in the current application.
    pub fn extend(
        driver: impl Into<String>,
        connector: impl Fn(&Container, &str, &Value) -> QueueResult<Arc<dyn Queue>>
        + Send
        + Sync
        + 'static,
    ) -> QueueResult<()> {
        Self::manager()?.extend(driver, connector);
        Ok(())
    }

    pub fn without_interruption_polling() -> QueueResult<()> {
        Self::manager()?.without_interruption_polling();
        Ok(())
    }

    pub async fn pause(connection: &str, queue: &str) -> QueueResult<()> {
        Self::manager()?.pause(connection, queue).await
    }

    pub async fn pause_for(connection: &str, queue: &str, ttl: Ttl) -> QueueResult<()> {
        Self::manager()?.pause_for(connection, queue, ttl).await
    }

    pub async fn pause_all() -> QueueResult<()> {
        Self::manager()?.pause_all().await
    }

    pub async fn resume(connection: &str, queue: &str) -> QueueResult<()> {
        Self::manager()?.resume(connection, queue).await
    }

    pub async fn resume_all() -> QueueResult<()> {
        Self::manager()?.resume_all().await
    }

    pub async fn is_paused(connection: &str, queue: &str) -> QueueResult<bool> {
        Self::manager()?.is_paused(connection, queue).await
    }

    pub async fn get_paused_queues<'a>(
        connection: &str,
        queues: &[&'a str],
    ) -> QueueResult<Vec<&'a str>> {
        Self::manager()?.get_paused_queues(connection, queues).await
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
