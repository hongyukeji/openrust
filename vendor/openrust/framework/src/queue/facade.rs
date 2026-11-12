use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::queue::{JobHandler, Queue},
    database::Transaction,
    support::facades::Facade,
};

use super::{DispatchOptions, DispatchResult, Payload, QueueError, QueueManager, QueueResult};

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

    pub fn connection(name: Option<&str>) -> QueueResult<Arc<dyn Queue>> {
        Self::manager()?.connection(name)
    }

    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub fn withoutInterruptionPolling() -> QueueResult<()> {
        Self::manager()?.withoutInterruptionPolling();
        Ok(())
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
