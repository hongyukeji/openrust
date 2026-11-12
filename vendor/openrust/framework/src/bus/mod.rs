//! Job batch state and its durable repository.

mod batch;
mod database_batch_repository;

pub use batch::Batch;
pub use database_batch_repository::DatabaseBatchRepository;
