//! Events emitted when a job batch changes state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{contracts::events::Event, support::facades::Event as Events};

use super::Batch;

#[derive(Serialize, Deserialize)]
#[serde(remote = "Batch")]
struct BatchSnapshot {
    id: String,
    name: String,
    total_jobs: u32,
    pending_jobs: u32,
    failed_jobs: u32,
    failed_job_ids: Vec<String>,
    options: serde_json::Map<String, serde_json::Value>,
    cancelled_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

macro_rules! batch_event {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub struct $name {
            #[serde(with = "BatchSnapshot")]
            pub batch: Batch,
        }

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

batch_event!(
    /// A batch and its jobs were committed to the queue.
    BatchDispatched
);
batch_event!(
    /// The worker recorded the first terminal outcome for a batch.
    BatchStarted
);
batch_event!(
    /// A batch reached zero pending jobs.
    BatchFinished
);

/// A batch was cancelled explicitly or by a terminal job failure.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchCanceled {
    #[serde(with = "BatchSnapshot")]
    pub batch: Batch,
    pub exception: Option<String>,
}

impl BatchCanceled {
    pub const NAME: &'static str = "BatchCanceled";
}

impl Event for BatchCanceled {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub(crate) async fn emit(event: impl Event) {
    let name = event.name().to_owned();
    if let Err(error) = Events::dispatch(event).await {
        let context = serde_json::Map::from_iter([
            ("event".into(), name.into()),
            ("error".into(), error.to_string().into()),
        ]);
        let _ = crate::log::Log::error("Batch event listener failed", &context);
    }
}
