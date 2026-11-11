//! Failed job storage contracts and providers.

mod database_uuid_failed_job_provider;
mod failed_job_provider_interface;
mod null_failed_job_provider;

pub use database_uuid_failed_job_provider::DatabaseUuidFailedJobProvider;
pub use failed_job_provider_interface::FailedJobProviderInterface;
pub(crate) use null_failed_job_provider::NullFailedJobProvider;

use super::{Payload, QueueError};

/// A failed job with its original payload and failure details.
pub struct FailedJob {
    pub id: String,
    pub connection: String,
    pub queue: String,
    pub payload: Payload,
    pub exception: String,
    pub failed_at: String,
}

impl FailedJob {
    pub(super) fn new(job: &dyn crate::contracts::queue::Job, error: &QueueError) -> Self {
        Self {
            id: job.uuid().into(),
            connection: job.get_connection_name().into(),
            queue: job.get_queue().into(),
            payload: job.payload().clone(),
            exception: error.to_string(),
            failed_at: timestamp(chrono::Utc::now()),
        }
    }
}

fn timestamp(time: chrono::DateTime<chrono::Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}
