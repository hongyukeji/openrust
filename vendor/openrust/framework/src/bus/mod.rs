//! Job batch state and its durable repository.

mod batch;
mod callback;
mod database_batch_repository;
pub mod events;

pub use batch::Batch;
pub use callback::BatchCallback;
pub(crate) use callback::{BatchPhase, BatchTransition};
pub use database_batch_repository::DatabaseBatchRepository;
pub(crate) use database_batch_repository::{PublicationKey, new_batch};

mod pending_batch;

pub use pending_batch::PendingBatch;

use crate::{
    queue::{Payload, QueueResult},
    support::facades::Queue,
};

/// The application-scoped job bus.
pub struct Bus;

#[allow(non_snake_case, reason = "Matches Laravel Bus")]
impl Bus {
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
