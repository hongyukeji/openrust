//! Database queue worker configuration.

use serde::Deserialize;

/// Bounds polling, job leases and graceful worker shutdown.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    /// Delay between empty queue polls, in milliseconds.
    pub poll_interval_millis: u64,
    /// Lease duration for a claimed job, in seconds.
    pub lease_secs: u64,
    /// Maximum execution time for one handler attempt, in seconds.
    pub job_timeout_secs: u64,
    /// Maximum number of concurrent handlers in one worker process.
    pub worker_concurrency: usize,
    /// Maximum time to wait for the current handler during shutdown, in seconds.
    pub shutdown_timeout_secs: u64,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            poll_interval_millis: 500,
            lease_secs: 60,
            job_timeout_secs: 300,
            worker_concurrency: 1,
            shutdown_timeout_secs: 10,
        }
    }
}
