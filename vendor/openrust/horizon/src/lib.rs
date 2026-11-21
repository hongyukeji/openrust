#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![allow(non_snake_case, reason = "Preserves Laravel Horizon API names")]

#[path = "../config/horizon.rs"]
mod configuration;
pub mod console;
mod dashboard;
mod notifications;
mod options;
mod provider;
mod repository;
mod signals;
mod supervisor;
mod tags;
pub use notifications::LongWaitDetected;
pub use tags::{TagCallback, Tags};
mod master_supervisor;

pub use dashboard::{Dashboard, Horizon};
pub use master_supervisor::{MasterSupervisor, SupervisorProcess};
pub use options::{AutoScaler, SupervisorOptions};
pub use provider::HorizonServiceProvider;
pub use repository::{
    JobDetails, JobRecord, JobRepository, MasterSupervisorRepository, Metrics, MetricsRepository,
    ProcessRepository, Repository, SupervisorRepository, TagRepository, WorkloadRepository,
};
pub use supervisor::{ProcessPool, Supervisor, WorkerProcess};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub enum Error {
    InvalidConfiguration,
    InvalidRecord,
    Redis,
    Queue,
    Process,
    MemoryLimitExceeded,
    Unavailable,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidConfiguration => "Invalid Horizon configuration",
            Self::InvalidRecord => "Invalid Horizon record",
            Self::Redis => "Horizon metadata storage failed",
            Self::Queue => "Horizon queue operation failed",
            Self::MemoryLimitExceeded => "Horizon memory limit exceeded",
            Self::Process => "Horizon worker process failed",
            Self::Unavailable => "Horizon service is unavailable",
        })
    }
}
impl std::error::Error for Error {}
pub(crate) fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
}
pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(HorizonServiceProvider)]
}
