#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod command_handler;
#[path = "../config/horizon.rs"]
mod configuration;
pub mod console;
mod dashboard;
mod events;
mod job_payload;
mod job_trim;
mod master_names;
mod notifications;
mod options;
mod process_memory;
mod process_output;
mod provider;
mod provisioning_plan;
mod repository;
mod signals;
mod silenced;
mod supervisor;
pub mod supervisor_commands;
mod tags;
pub use notifications::{
    LongWaitDetected, LongWaitDetectedNotification, LongWaitNotificationFactory, MonitorWaitTimes,
    SendNotification,
};
pub use tags::{ModelTagCallback, TagCallback, Tags};
mod master_supervisor;

pub use command_handler::{
    CommandFuture, MasterSupervisorCommandHandler, SupervisorCommandHandler,
};
pub use dashboard::{Dashboard, Horizon};
pub use events::{
    JobDeleted, JobFailed, JobPending, JobPushed, JobReleased, JobReserved, JobsMigrated,
};
pub use job_payload::{JobPayload, JobPayloadMetadata};
pub use job_trim::{TrimFailedJobs, TrimMonitoredJobs, TrimRecentJobs};
pub use master_supervisor::{
    AddSupervisor, ExpireSupervisors, MasterSupervisor, MasterSupervisorDeployed,
    MasterSupervisorLooped, MasterSupervisorReviving, SupervisorProcess,
    SupervisorProcessRestarting,
};
pub use options::{AutoScaler, SupervisorOptions};
pub use process_memory::{
    MasterSupervisorOutOfMemory, MonitorMasterSupervisorMemory, MonitorSupervisorMemory,
    SupervisorOutOfMemory,
};
pub use provider::HorizonServiceProvider;
pub use provisioning_plan::ProvisioningPlan;
pub use repository::{
    CommandQueueTarget, HorizonCommandQueue, JobDetails, JobRecord, JobRepository,
    MasterSupervisorRepository, Metrics, MetricsRepository, ProcessRepository, QueuedCommand,
    RedisHorizonCommandQueue, Repository, SupervisorRepository, TagRepository, WorkloadRepository,
};
pub use silenced::Silenced;
pub use supervisor::{
    ProcessPool, Supervisor, SupervisorLooped, UnableToLaunchProcess, WorkerProcess,
    WorkerProcessRestarting,
};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub enum Error {
    InvalidConfiguration,
    InvalidRecord,
    Redis,
    Queue,
    Process,
    MemoryLimitExceeded,
    MasterAlreadyRunning,
    MasterOwnershipLost,
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
            Self::MasterAlreadyRunning => "A master supervisor is already running on this machine.",
            Self::MasterOwnershipLost => "Horizon Master 已失去名称所有权。",
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
