use super::BatchJob;

use jiff::Timestamp;
use serde::{Serialize, Serializer, ser::SerializeStruct};
use serde_json::{Map, Value};

use crate::{
    queue::{Payload, QueueResult},
    support::facades::Queue,
};

/// A snapshot of the persisted state of a job batch.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub id: String,
    pub name: String,
    pub total_jobs: u32,
    pub pending_jobs: u32,
    pub failed_jobs: u32,
    pub failed_job_ids: Vec<String>,
    pub options: Map<String, Value>,
    pub cancelled_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

impl Batch {
    /// Appends jobs to the persisted batch and returns its updated snapshot.
    #[allow(clippy::should_implement_trait, reason = "保留既定公开接口及命名")]
    pub async fn add(&self, jobs: impl IntoIterator<Item = Payload>) -> QueueResult<Option<Self>> {
        let jobs = jobs.into_iter().map(BatchJob::Job).collect();
        Queue::manager()?.append_batch(&self.id, jobs).await
    }

    /// Appends a sequence to the persisted batch.
    pub async fn add_chain(
        &self,
        jobs: impl IntoIterator<Item = Payload>,
    ) -> QueueResult<Option<Self>> {
        let chain = BatchJob::Chain(jobs.into_iter().collect());
        Queue::manager()?.append_batch(&self.id, vec![chain]).await
    }

    pub fn processed_jobs(&self) -> u32 {
        self.total_jobs.saturating_sub(self.pending_jobs)
    }

    pub fn progress(&self) -> u32 {
        if self.total_jobs == 0 {
            return 0;
        }
        let total = u64::from(self.total_jobs);
        let processed = u64::from(self.processed_jobs());
        ((100 * processed + total / 2) / total) as u32
    }

    pub fn finished(&self) -> bool {
        self.finished_at.is_some()
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled_at.is_some()
    }

    pub fn canceled(&self) -> bool {
        self.cancelled()
    }

    pub fn has_failures(&self) -> bool {
        self.failed_jobs != 0
    }

    pub fn allows_failures(&self) -> bool {
        self.options
            .get("allowFailures")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}

impl Serialize for Batch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut batch = serializer.serialize_struct("Batch", 11)?;
        batch.serialize_field("id", &self.id)?;
        batch.serialize_field("name", &self.name)?;
        batch.serialize_field("totalJobs", &self.total_jobs)?;
        batch.serialize_field("pendingJobs", &self.pending_jobs)?;
        batch.serialize_field("processedJobs", &self.processed_jobs())?;
        batch.serialize_field("progress", &self.progress())?;
        batch.serialize_field("failedJobs", &self.failed_jobs)?;
        batch.serialize_field("options", &self.options)?;
        batch.serialize_field("createdAt", &self.created_at)?;
        batch.serialize_field("cancelledAt", &self.cancelled_at)?;
        batch.serialize_field("finishedAt", &self.finished_at)?;
        batch.end()
    }
}
