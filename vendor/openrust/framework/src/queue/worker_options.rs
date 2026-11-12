use std::time::Duration;

pub(super) const MEBIBYTE: u64 = 1024 * 1024;

const fn default_memory() -> u64 {
    128 * MEBIBYTE
}

const fn stop_on_lost_connection() -> bool {
    true
}

/// Controls a queue worker's execution and stopping limits.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkerOptions {
    pub tries: u32,
    pub timeout: Duration,
    pub backoff: Vec<Duration>,
    pub sleep: Duration,
    pub max_jobs: Option<u64>,
    pub max_time: Option<Duration>,
    pub stop_when_empty: bool,
    #[serde(default = "stop_on_lost_connection")]
    pub stop_on_lost_connection: bool,
    /// Resident memory threshold in bytes. Zero disables the limit.
    #[serde(default = "default_memory")]
    pub memory: u64,
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            tries: 1,
            timeout: Duration::from_secs(60),
            backoff: Vec::new(),
            sleep: Duration::from_secs(3),
            max_jobs: None,
            max_time: None,
            stop_when_empty: false,
            stop_on_lost_connection: stop_on_lost_connection(),
            memory: default_memory(),
        }
    }
}
