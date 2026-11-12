use std::time::Duration;

pub(super) const MEBIBYTE: u64 = 1024 * 1024;

fn default_name() -> String {
    "default".into()
}

const fn default_memory() -> u64 {
    128 * MEBIBYTE
}

const fn stop_by_default() -> bool {
    true
}

const fn default_timed_out_exit_code() -> u8 {
    1
}

/// Controls a queue worker's execution and stopping limits.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkerOptions {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default)]
    pub force: bool,
    pub tries: u32,
    pub timeout: Duration,
    pub backoff: Vec<Duration>,
    pub sleep: Duration,
    pub max_jobs: Option<u64>,
    pub max_time: Option<Duration>,
    pub stop_when_empty: bool,
    #[serde(default = "stop_by_default")]
    pub stop_on_lost_connection: bool,
    #[serde(default = "stop_by_default")]
    pub kill_on_timeout: bool,
    #[serde(default = "default_timed_out_exit_code")]
    pub timed_out_exit_code: u8,
    /// Resident memory threshold in bytes. Zero disables the limit.
    #[serde(default = "default_memory")]
    pub memory: u64,
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            name: default_name(),
            force: false,
            tries: 1,
            timeout: Duration::from_secs(60),
            backoff: Vec::new(),
            sleep: Duration::from_secs(3),
            max_jobs: None,
            max_time: None,
            stop_when_empty: false,
            stop_on_lost_connection: stop_by_default(),
            kill_on_timeout: stop_by_default(),
            timed_out_exit_code: default_timed_out_exit_code(),
            memory: default_memory(),
        }
    }
}
