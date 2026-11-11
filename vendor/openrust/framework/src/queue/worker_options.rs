use std::time::Duration;

/// Controls a queue worker's execution and stopping limits.
pub struct WorkerOptions {
    pub tries: u32,
    pub timeout: Duration,
    pub backoff: Vec<Duration>,
    pub sleep: Duration,
    pub max_jobs: Option<u64>,
    pub max_time: Option<Duration>,
    pub stop_when_empty: bool,
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
        }
    }
}
