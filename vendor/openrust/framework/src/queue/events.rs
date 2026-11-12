//! Queue and worker pause transitions.

use super::{Payload, WorkerOptions, WorkerStopReason};
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
    /// A worker completed a graceful stop.
    WorkerStopping {
        status: i32,
        worker_options: WorkerOptions,
        reason: WorkerStopReason,
        jobs_processed: u64,
        last_job_processed_at: Option<u64>,
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
