//! Job batch state and its durable repository.

mod batch;
mod callback;
mod chained_batch;
pub(crate) use chained_batch::{BatchContinuation, ChainedBatch};
mod database_batch_repository;
pub mod events;

pub use batch::Batch;
pub use callback::BatchCallback;
pub(crate) use callback::{BatchPhase, BatchTransition};
pub use database_batch_repository::DatabaseBatchRepository;
pub(crate) use database_batch_repository::new_batch;

mod pending_batch;
mod pending_chain;

pub(crate) use pending_batch::BatchJob;
pub use pending_batch::PendingBatch;
pub use pending_chain::{ChainCallback, ChainCondition, PendingChain};

use crate::{
    database::Transaction,
    queue::{DispatchOptions, DispatchResult, Payload, QueueResult},
    support::facades::Queue,
};

/// The application-scoped job bus.
pub struct Bus;

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Bus {
    /// Dispatches a queueable payload through the application's queue manager.
    pub async fn dispatch(
        payload: Payload,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<DispatchResult> {
        Queue::dispatch(payload, options, transaction).await
    }

    pub async fn dispatchToQueue(
        payload: Payload,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<DispatchResult> {
        Self::dispatch(payload, options, transaction).await
    }

    /// Executes through the sync queue, including job middleware and queue events.
    pub async fn dispatchSync(
        payload: Payload,
        mut options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<DispatchResult> {
        options.connection = Some("sync".into());
        options.delay = None;
        Self::dispatch(payload, options, transaction).await
    }

    /// Calls the registered handler directly in the current process.
    pub async fn dispatchNow(payload: Payload) -> QueueResult<()> {
        Queue::manager()?.dispatch_now(&payload).await
    }

    /// Dispatches the supplied jobs in order using their individual options.
    pub async fn bulk(
        jobs: impl IntoIterator<Item = (Payload, DispatchOptions)>,
    ) -> QueueResult<Vec<DispatchResult>> {
        let manager = Queue::manager()?;
        let mut results = Vec::new();
        for (payload, options) in jobs {
            results.push(manager.dispatch(payload, options, None).await?);
        }
        Ok(results)
    }

    /// Runs the job on the sync queue during HTTP termination.
    pub async fn dispatchAfterResponse(
        payload: Payload,
        options: DispatchOptions,
    ) -> QueueResult<()> {
        let manager = Queue::manager()?;
        if !manager.bus_after_response() {
            return Self::dispatchSync(payload, options, None).await.map(|_| ());
        }
        let mut options = options;
        options.connection = Some("sync".into());
        options.delay = None;
        let callback = crate::concurrency::enqueue_deferred::<crate::queue::QueueError>(Box::pin(
            async move {
                manager.dispatch(payload, options, None).await?;
                Ok(())
            },
        ))
        .map_err(crate::queue::backend)?;
        let _ = callback.always();
        Ok(())
    }

    pub fn withDispatchingAfterResponses() -> QueueResult<()> {
        Queue::manager()?.set_bus_after_response(true);
        Ok(())
    }

    pub fn withoutDispatchingAfterResponses() -> QueueResult<()> {
        Queue::manager()?.set_bus_after_response(false);
        Ok(())
    }

    pub fn pipe_through(
        pipes: Vec<std::sync::Arc<dyn crate::queue::middleware::JobMiddleware>>,
    ) -> QueueResult<()> {
        Queue::manager()?.set_bus_pipes(pipes);
        Ok(())
    }

    pub fn hasCommandHandler(name: &str) -> QueueResult<bool> {
        Ok(Self::getCommandHandler(name)?.is_some())
    }

    pub fn getCommandHandler(
        name: &str,
    ) -> QueueResult<Option<std::sync::Arc<dyn crate::contracts::queue::JobHandler>>> {
        Ok(Queue::manager()?.command_handler(name))
    }

    pub fn map(
        handlers: impl IntoIterator<
            Item = (
                String,
                std::sync::Arc<dyn crate::contracts::queue::JobHandler>,
            ),
        >,
    ) -> QueueResult<()> {
        let manager = Queue::manager()?;
        for (name, handler) in handlers {
            manager.register(&name, handler)?;
        }
        Ok(())
    }

    pub fn chain(jobs: impl IntoIterator<Item = Payload>) -> PendingChain {
        PendingChain::new(jobs)
    }

    pub fn registerChainCallback(
        name: impl Into<String>,
        callback: ChainCallback,
    ) -> QueueResult<()> {
        Queue::manager()?.register_chain_callback(name.into(), callback)
    }

    pub fn batch(jobs: impl IntoIterator<Item = Payload>) -> PendingBatch {
        PendingBatch::new(jobs)
    }

    pub fn registerBatchCallback(
        name: impl Into<String>,
        callback: BatchCallback,
    ) -> QueueResult<()> {
        Queue::manager()?.register_batch_callback(name.into(), callback)
    }

    pub async fn findBatch(id: &str) -> QueueResult<Option<Batch>> {
        Queue::manager()?.batch_repository()?.find(id).await
    }

    pub async fn cancelBatch(id: &str) -> QueueResult<Option<Batch>> {
        let (batch, changed) = Queue::manager()?
            .batch_repository()?
            .cancel_once(id)
            .await?;
        if changed && let Some(batch) = &batch {
            events::emit(events::BatchCanceled {
                batch: batch.clone(),
                exception: None,
            })
            .await;
        }
        Ok(batch)
    }
}
