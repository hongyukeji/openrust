use super::{Pulse, Recorders};
use jiff::Timestamp;
use openrust::{contracts::events::Event, queue::events::*};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex, Weak},
};

#[derive(Clone, Eq, Hash, PartialEq)]
struct Attempt {
    connection: String,
    queue: String,
    id: String,
    number: u32,
}

impl Attempt {
    fn new(connection: &str, job: &JobSnapshot) -> Self {
        Self {
            connection: connection.to_owned(),
            queue: job.queue.clone(),
            id: job.id.clone(),
            number: job.attempts,
        }
    }
}

#[derive(Default)]
pub(super) struct SlowJobStarts(Mutex<HashMap<Attempt, Arc<Timestamp>>>);

struct AttemptCleanup {
    starts: Weak<SlowJobStarts>,
    attempt: Attempt,
    started_at: Arc<Timestamp>,
}

impl Drop for AttemptCleanup {
    fn drop(&mut self) {
        if let Some(starts) = self.starts.upgrade() {
            let mut entries = starts.0.lock().unwrap();
            if entries
                .get(&self.attempt)
                .is_some_and(|started_at| Arc::ptr_eq(started_at, &self.started_at))
            {
                entries.remove(&self.attempt);
            }
        }
    }
}

impl SlowJobStarts {
    pub(super) async fn observe(
        self: &Arc<Self>,
        event: &dyn Event,
        recorder: &Recorders,
        pulse: &Pulse,
        timestamp: Timestamp,
    ) {
        if let Some(e) = event.downcast_ref::<JobProcessing>() {
            if e.connection_name != "sync" {
                let attempt = Attempt::new(&e.connection_name, &e.job);
                let started_at = Arc::new(timestamp);
                let cleanup = AttemptCleanup {
                    starts: Arc::downgrade(self),
                    attempt: attempt.clone(),
                    started_at: started_at.clone(),
                };
                // Worker 即使在超时或取消时没有运行回调，也会释放本次尝试的回调集合。
                // 无生命周期的手动事件仍由后续终止事件清理。
                let _ = openrust::support::defer(
                    async move {
                        drop(cleanup);
                        Ok::<(), Infallible>(())
                    },
                    None,
                    true,
                );
                self.0.lock().unwrap().insert(attempt, started_at);
            }
            return;
        }

        let (connection, job) = if let Some(e) = event.downcast_ref::<JobProcessed>() {
            (&e.connection_name, &e.job)
        } else if let Some(e) = event.downcast_ref::<JobFailed>() {
            (&e.connection_name, &e.job)
        } else if let Some(e) = event.downcast_ref::<JobReleasedAfterException>() {
            (&e.connection_name, &e.job)
        } else {
            // 清理未发出可计入慢任务的终止事件的尝试，例如 Worker 前置检查失败。
            if let Some(e) = event.downcast_ref::<JobAttempted>() {
                self.take(&e.connection_name, &e.job);
            }
            return;
        };

        let Some(started_at) = self.take(connection, job) else {
            return;
        };
        let duration = timestamp.as_millisecond() - started_at.as_millisecond();
        let name = job.payload.display_name().to_owned();
        let recorder = recorder.clone();
        pulse
            .lazy(move |pulse| {
                Box::pin(async move {
                    if recorder.accepts("SlowJobs", &name, Some(duration), None)? {
                        pulse
                            .record(
                                "slow_job",
                                name,
                                Some(duration),
                                Some(timestamp.as_second()),
                            )
                            .await
                            .max()
                            .count();
                    }
                    Ok(())
                })
            })
            .await;
    }

    fn take(&self, connection: &str, job: &JobSnapshot) -> Option<Arc<Timestamp>> {
        if connection == "sync" {
            return None;
        }
        self.0
            .lock()
            .unwrap()
            .remove(&Attempt::new(connection, job))
    }
}
