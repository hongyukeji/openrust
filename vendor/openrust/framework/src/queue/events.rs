//! Queue and worker lifecycle events.

use std::{error::Error as _, time::Duration};

use super::{Payload, QueueError, WorkerOptions, WorkerStopReason, failed::FailedJob};
use crate::{
    cache::Ttl,
    contracts::{events::Event, queue::Job},
};

macro_rules! queue_event {
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
        pub struct $name { $($(#[$field_meta])* pub $field: $ty),* }

        impl $name {
            pub const NAME: &'static str = stringify!($name);
        }

        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }
        }
    };
}

queue_event!(
    /// A monitored queue has reached its configured size threshold.
    QueueBusy { connection_name: String, queue: String, size: u64 }
);

queue_event!(
    /// An individual queue pause was written.
    QueuePaused { connection: String, queue: String, ttl: Option<Ttl> }
);
queue_event!(
    /// An individual queue pause was removed.
    QueueResumed { connection: String, queue: String }
);
queue_event!(
    /// The global pause was written.
    QueuesPaused {}
);
queue_event!(
    /// The global pause was removed.
    QueuesResumed {}
);
queue_event!(
    /// A worker observed a queue entering a pause.
    WorkerQueuePaused { connection: String, queue: String }
);
queue_event!(
    /// A worker observed a queue leaving a pause.
    WorkerQueueResumed { connection: String, queue: String }
);

queue_event!(
    /// A daemon worker is starting its polling loop.
    WorkerStarting {
        connection_name: String,
        queue: String,
        worker_options: WorkerOptions,
    }
);

queue_event!(
    /// A daemon is about to poll the queue. A listener may veto the iteration.
    Looping {
        connection_name: String,
        queue: String,
        worker_options: WorkerOptions,
    }
);

queue_event!(
    /// A daemon worker could not reserve a job.
    WorkerIdle {
        connection_name: String,
        queue: String,
        worker_options: WorkerOptions,
    }
);

queue_event!(
    /// A daemon received a process signal requesting a graceful stop.
    WorkerInterrupted {
        signal: i32,
        connection_name: String,
        queue: String,
        worker_options: WorkerOptions,
    }
);

queue_event!(
    /// A daemon received a process signal to pause polling.
    WorkerPausing {
        connection_name: String,
        queue: String,
        worker_options: WorkerOptions,
    }
);

queue_event!(
    /// A daemon received a process signal to resume polling.
    WorkerResuming {
        connection_name: String,
        queue: String,
        worker_options: WorkerOptions,
    }
);

queue_event!(
    /// A worker completed a graceful stop.
    WorkerStopping {
        status: i32,
        worker_options: WorkerOptions,
        reason: WorkerStopReason,
        jobs_processed: u64,
        last_job_processed_at: Option<u64>,
        /// Resident memory at shutdown in MiB.
        #[serde(default)]
        memory_usage: Option<f64>,
        connection_name: String,
        queue: String,
    }
);

queue_event!(
    /// A worker is about to reserve a job from the selected queues.
    JobPopping {
        connection_name: String,
        queue: Option<String>,
    }
);

/// A reservation's job data as observed by a worker event listener.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JobSnapshot {
    pub id: String,
    pub queue: String,
    pub attempts: u32,
    pub payload: Payload,
}

impl JobSnapshot {
    pub(super) fn from_job(job: &dyn Job) -> Self {
        Self {
            id: job.get_job_id().to_owned(),
            queue: job.get_queue().to_owned(),
            attempts: job.attempts(),
            payload: job.payload().clone(),
        }
    }
}

queue_event!(
    /// A worker reserved a job before executing it.
    JobPopped {
        connection_name: String,
        job: JobSnapshot,
    }
);

queue_event!(
    /// The worker is about to execute an attempt.
    JobProcessing {
        connection_name: String,
        job: JobSnapshot,
    }
);

queue_event!(
    /// A job attempt returned successfully.
    JobProcessed {
        connection_name: String,
        job: JobSnapshot,
        duration: f64,
    }
);

queue_event!(
    /// The worker finished an attempt, including its retry or failure handling.
    JobAttempted {
        connection_name: String,
        job: JobSnapshot,
        exception: Option<String>,
        duration: Option<f64>,
    }
);

impl JobAttempted {
    pub fn successful(&self) -> bool {
        self.exception.is_none()
    }
}

queue_event!(
    /// A job attempt raised an exception.
    JobExceptionOccurred {
        connection_name: String,
        job: JobSnapshot,
        exception: String,
    }
);

queue_event!(
    /// A successful job attempt explicitly released its reservation.
    JobReleased {
        connection_name: String,
        job: JobSnapshot,
    }
);

queue_event!(
    /// The worker released a job for retry after an exception.
    JobReleasedAfterException {
        connection_name: String,
        job: JobSnapshot,
        backoff: std::time::Duration,
        exception: String,
    }
);

queue_event!(
    /// The worker permanently failed a job.
    JobFailed {
        connection_name: String,
        job: JobSnapshot,
        exception: String,
    }
);

queue_event!(
    /// Artisan requested that a stored failure be put back on its original queue.
    JobRetryRequested { job: FailedJob }
);

queue_event!(
    /// The worker cancelled a job attempt at its configured timeout.
    JobTimedOut {
        connection_name: String,
        job: JobSnapshot,
        timeout: std::time::Duration,
    }
);

queue_event!(
    /// A running job handled an interruption from its worker.
    JobInterrupted {
        connection_name: String,
        job: JobSnapshot,
        signal: i32,
    }
);

pub(super) fn attempt_duration(elapsed: Duration) -> f64 {
    (elapsed.as_secs_f64() * 100_000.0).round() / 100.0
}

pub(super) fn attempt_exception_message(error: &QueueError) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        message.push_str(": ");
        message.push_str(&error.to_string());
        source = error.source();
    }
    message
}
