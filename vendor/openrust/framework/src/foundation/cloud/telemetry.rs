use super::Queue;
use crate::{
    contracts::{container::Container as _, queue::Job},
    queue::{Delay, JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueResult},
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError, Weak,
        atomic::{AtomicU8, Ordering},
    },
};
#[derive(Default)]
pub(crate) struct Execution(Mutex<HashMap<usize, Processing>>);
struct Processing {
    details: ProcessingJobDetails,
    status: Arc<AtomicU8>,
    _identity: Weak<()>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct ProcessingJobDetails {
    pub queue: String,
    pub attempts: u32,
    pub started_at: jiff::Timestamp,
}
impl Queue {
    fn execution(&self) -> QueueResult<Arc<Execution>> {
        if let Some(execution) = super::worker::execution(self) {
            return Ok(execution);
        }
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        if owner.has_active_scope() {
            Ok(owner.make::<Execution>()?)
        } else {
            Ok(self.execution.clone())
        }
    }
    fn key(&self) -> usize {
        Arc::as_ptr(&self.identity).addr()
    }
    async fn emit(&self, payload: serde_json::Value) -> QueueResult<()> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        if owner.bound::<super::Events>() {
            owner.make::<super::Events>()?.emit(&payload).await;
        }
        Ok(())
    }
    pub async fn finish_queueing_job(&self, queue: Option<&str>) -> QueueResult<()> {
        self.emit(json!({"_cloud_event":"queue","timestamp":timestamp(jiff::Timestamp::now()),"type":"queued","queue":self.normalize_queue(queue)?})).await
    }
    pub async fn finish_processing_job(
        &self,
        default: Option<&str>,
        at: Option<jiff::Timestamp>,
    ) -> QueueResult<()> {
        let processing = self
            .execution()?
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key());
        let Some(processing) = processing else {
            return Ok(());
        };
        let at = at.unwrap_or_else(jiff::Timestamp::now);
        let kind = match processing.status.load(Ordering::Acquire) {
            3 => "failed",
            2 => "released",
            _ => default.unwrap_or("processed"),
        };
        let duration = (i128::from(at.as_microsecond())
            - i128::from(processing.details.started_at.as_microsecond()))
            / 1000;
        self.emit(json!({"_cloud_event":"queue","timestamp":timestamp(at),"type":kind,"queue":processing.details.queue,"duration_ms":i64::try_from(duration).map_err(|_|QueueError::InvalidConfiguration)?})).await
    }
    pub fn processing_job_details(&self) -> QueueResult<Option<ProcessingJobDetails>> {
        Ok(self
            .execution()?
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&self.key())
            .map(|p| p.details.clone()))
    }
    pub(crate) async fn start_processing_job(
        &self,
        queue: Option<&str>,
        job: Option<Box<dyn Job>>,
    ) -> QueueResult<Option<Box<dyn Job>>> {
        let Some(job) = job else { return Ok(None) };
        let details = ProcessingJobDetails {
            queue: self.normalize_queue(queue)?,
            attempts: job.attempts(),
            started_at: jiff::Timestamp::now(),
        };
        let status = Arc::new(AtomicU8::new(if job.has_failed() {
            3
        } else if job.is_released() {
            2
        } else {
            0
        }));
        self.execution()?
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                self.key(),
                Processing {
                    details: details.clone(),
                    status: status.clone(),
                    _identity: Arc::downgrade(&self.identity),
                },
            );
        self.emit(json!({"_cloud_event":"queue","timestamp":timestamp(details.started_at),"type":"started","queue":details.queue})).await?;
        Ok(Some(Box::new(ObservedJob { inner: job, status })))
    }
}
fn timestamp(at: jiff::Timestamp) -> String {
    at.to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d %H:%M:%S%.6f")
        .to_string()
}
struct ObservedJob {
    inner: Box<dyn Job>,
    status: Arc<AtomicU8>,
}
impl ObservedJob {
    fn released(&self) {
        // 状态仅有0（处理中）、2（重新入队）、3（失败）；重新入队不能覆盖失败。
        self.status.fetch_max(2, Ordering::AcqRel);
    }
}
impl Job for ObservedJob {
    fn uuid(&self) -> &str {
        self.inner.uuid()
    }
    fn get_job_id(&self) -> &str {
        self.inner.get_job_id()
    }
    fn get_connection_name(&self) -> &str {
        self.inner.get_connection_name()
    }
    fn get_queue(&self) -> &str {
        self.inner.get_queue()
    }
    fn payload(&self) -> &Payload {
        self.inner.payload()
    }
    fn options(&self) -> &JobOptions {
        self.inner.options()
    }
    fn attempts(&self) -> u32 {
        self.inner.attempts()
    }
    fn is_deleted(&self) -> bool {
        self.inner.is_deleted()
    }
    fn is_released(&self) -> bool {
        self.inner.is_released()
    }
    fn has_failed(&self) -> bool {
        self.inner.has_failed()
    }
    fn mark_as_failed(&mut self) {
        self.status.store(3, Ordering::Release);
        self.inner.mark_as_failed()
    }
    fn supports_isolated_execution(&self) -> bool {
        self.inner.supports_isolated_execution()
    }
    fn fire<'a>(&'a mut self, context: &'a mut JobContext<'_>) -> QueueFuture<'a, QueueResult<()>> {
        self.inner.fire(context)
    }
    fn fail<'a>(&'a mut self, cause: Option<&'a QueueError>) -> QueueFuture<'a, QueueResult<()>> {
        self.status.store(3, Ordering::Release);
        self.inner.fail(cause)
    }
    fn release(&mut self, delay: std::time::Duration) -> QueueFuture<'_, QueueResult<()>> {
        self.released();
        self.inner.release(delay)
    }
    fn advance_chain<'a>(&'a mut self, next: &'a Payload) -> QueueFuture<'a, QueueResult<()>> {
        self.inner.advance_chain(next)
    }
    fn advance_chain_to<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        self.inner.advance_chain_to(next, queue, delay)
    }
    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>> {
        self.inner.delete()
    }
}
