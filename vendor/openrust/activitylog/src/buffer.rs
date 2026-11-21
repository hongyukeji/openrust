use crate::{ActivityRecord, ActivityRepository, PreparedActivity, Result};
use std::sync::{Arc, Mutex};

/// A buffer belongs to one request or job. Explicit callers must await `flush`.
/// Records retain no database ID until insertion, matching deferred persistence.
pub struct ActivityBuffer {
    repository: Arc<ActivityRepository>,
    pending: Mutex<Vec<PreparedActivity>>,
    flushing: tokio::sync::Mutex<()>,
}
impl ActivityBuffer {
    pub fn new(repository: Arc<ActivityRepository>) -> Self {
        Self {
            repository,
            pending: Mutex::new(Vec::new()),
            flushing: tokio::sync::Mutex::new(()),
        }
    }
    pub fn add(&self, activity: PreparedActivity) -> ActivityRecord {
        let record = activity.record().clone();
        self.pending.lock().unwrap().push(activity);
        record
    }
    pub fn hasPending(&self) -> bool {
        self.count() != 0
    }
    pub fn count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
    pub async fn flush(&self) -> Result<usize> {
        let _guard = self.flushing.lock().await;
        let records = self.pending.lock().unwrap().clone();
        if records.is_empty() {
            return Ok(0);
        }
        let count = records.len();
        self.repository.insertBuffered(records).await?;
        // Additions while SQL was running remain pending for the next flush.
        self.pending.lock().unwrap().drain(..count);
        Ok(count)
    }
}
