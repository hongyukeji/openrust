use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeStruct};
use serde_json::{Map, Value};

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
    pub cancelled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[allow(non_snake_case, reason = "Matches Laravel batch methods")]
impl Batch {
    pub fn processedJobs(&self) -> u32 {
        self.total_jobs.saturating_sub(self.pending_jobs)
    }

    pub fn progress(&self) -> u32 {
        if self.total_jobs == 0 {
            return 0;
        }
        let total = u64::from(self.total_jobs);
        let processed = u64::from(self.processedJobs());
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

    pub fn hasFailures(&self) -> bool {
        self.failed_jobs != 0
    }

    pub fn allowsFailures(&self) -> bool {
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
        batch.serialize_field("processedJobs", &self.processedJobs())?;
        batch.serialize_field("progress", &self.progress())?;
        batch.serialize_field("failedJobs", &self.failed_jobs)?;
        batch.serialize_field("options", &self.options)?;
        batch.serialize_field("createdAt", &self.created_at)?;
        batch.serialize_field("cancelledAt", &self.cancelled_at)?;
        batch.serialize_field("finishedAt", &self.finished_at)?;
        batch.end()
    }
}
