//! Job batch state and its durable repository.

mod batch;
mod database_batch_repository;

pub use batch::Batch;
pub use database_batch_repository::DatabaseBatchRepository;

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

    pub async fn findBatch(id: &str) -> QueueResult<Option<Batch>> {
        Queue::manager()?.batch_repository()?.find(id).await
    }

    pub async fn cancelBatch(id: &str) -> QueueResult<Option<Batch>> {
        Queue::manager()?.batch_repository()?.cancel(id).await
    }
}
