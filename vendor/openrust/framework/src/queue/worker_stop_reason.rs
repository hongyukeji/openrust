//! Reasons for a graceful queue worker exit.

/// The condition that stopped a queue worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum WorkerStopReason {
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "max_jobs")]
    MaxJobsExceeded,
    #[serde(rename = "memory")]
    MaxMemoryExceeded,
    #[serde(rename = "max_time")]
    MaxTimeExceeded,
    #[serde(rename = "empty")]
    QueueEmpty,
    #[serde(rename = "empty_for")]
    QueueEmptyFor,
    #[serde(rename = "restart_signal")]
    ReceivedRestartSignal,
}

impl WorkerStopReason {
    pub const fn description(self) -> &'static str {
        match self {
            Self::Interrupted => "Interrupted",
            Self::MaxMemoryExceeded => "Memory limit exceeded",
            Self::MaxJobsExceeded => "Maximum jobs exceeded",
            Self::MaxTimeExceeded => "Maximum run time exceeded",
            Self::QueueEmpty => "Queue empty",
            Self::QueueEmptyFor => "Queue empty for the configured duration",
            Self::ReceivedRestartSignal => "Received restart signal",
        }
    }
}
