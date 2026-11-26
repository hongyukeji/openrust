//! Failed job storage contracts and providers.

mod database_uuid_failed_job_provider;
mod failed_job_provider_interface;
mod null_failed_job_provider;
mod prunable_failed_job_provider;

pub use database_uuid_failed_job_provider::DatabaseUuidFailedJobProvider;
pub use failed_job_provider_interface::{FailedJobProviderInterface, FailedJobRetryStream};
pub(crate) use null_failed_job_provider::NullFailedJobProvider;
pub use prunable_failed_job_provider::PrunableFailedJobProvider;

use super::{Payload, QueueError};

/// A failed job with its original payload and failure details.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
            // 与队列事件共用错误链格式，失败存储不能丢失实际业务错误。
            exception: super::events::attempt_exception_message(error),
            failed_at: timestamp(jiff::Timestamp::now()),
        }
    }
}

fn timestamp(time: jiff::Timestamp) -> String {
    time.strftime("%Y-%m-%d %H:%M:%S").to_string()
}

/// 按连接和队列筛选失败任务数的可选能力。
pub trait CountableFailedJobProvider: Send + Sync {
    fn count<'a>(
        &'a self,
        connection: Option<&'a str>,
        queue: Option<&'a str>,
    ) -> super::QueueFuture<'a, super::QueueResult<u64>>;
}
