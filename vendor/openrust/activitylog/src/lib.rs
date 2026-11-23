#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod activity;
mod buffer;
pub use buffer::ActivityBuffer;
mod change_detector;
pub use change_detector::ChangeDetector;
pub mod actions;
#[path = "../config/activitylog.rs"]
mod configuration;
pub mod facades;
mod logger;
#[path = "../database/migrations/2026_11_21_000001_create_activity_log_table.rs"]
mod migration;
pub mod models;
mod provider;
mod repository;
pub mod support;

pub use activity::{Activity, ActivityRecord, IntoLogName, Subject};
pub use logger::{ActivityLogContext, ActivityLogger, PendingActivityLog};
pub use migration::CreateActivityLogTable;
pub use provider::ActivitylogServiceProvider;
pub use repository::{ActivityQuery, ActivityRepository, PreparedActivity, Redaction};
pub use support::{ActivityLogStatus, CauserResolver, LogOptions};

#[derive(Debug)]
pub enum Error {
    InvalidActivity,
    InvalidConfiguration,
    Database(openrust::database::DatabaseError),
}
pub type Result<T> = std::result::Result<T, Error>;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidActivity => "Invalid activity or activity payload limit exceeded",
            Self::InvalidConfiguration => "Invalid activity log configuration",
            Self::Database(_) => "Activity log database operation failed",
        })
    }
}
impl std::error::Error for Error {}
impl From<openrust::database::DatabaseError> for Error {
    fn from(value: openrust::database::DatabaseError) -> Self {
        Self::Database(value)
    }
}
pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(ActivitylogServiceProvider)]
}

/// Starts a fresh activity using the current application package binding.
pub fn activity()
-> std::result::Result<PendingActivityLog, openrust::contracts::container::BindingResolutionError> {
    facades::Activitylog::activity()
}
