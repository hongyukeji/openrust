#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![allow(non_snake_case, reason = "Preserves Laravel Telescope public names")]

mod entry;
pub use entry::{
    EntryResult, EntryType, EntryUpdate, IncomingDumpEntry, IncomingEntry, IncomingExceptionEntry,
};
mod console;
pub mod contracts;
pub mod facades;
pub mod http;
pub mod jobs;
#[path = "../database/migrations/2018_08_08_100000_create_telescope_entries_table.rs"]
mod migration;
pub mod storage;
pub use migration::CreateTelescopeEntriesTable;
#[path = "../config/telescope.rs"]
mod configuration;
mod provider;
pub use provider::TelescopeServiceProvider;
mod telescope;
pub use telescope::Telescope;
pub mod watchers;
pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(TelescopeServiceProvider)]
}

#[derive(Debug)]
pub enum Error {
    InvalidEntry,
    InvalidConfiguration,
    NotFound,
    Database(openrust::database::DatabaseError),
    Hostname(std::io::Error),
    Queue(openrust::queue::QueueError),
}
pub type Result<T> = std::result::Result<T, Error>;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidEntry => "Invalid Telescope entry",
            Self::InvalidConfiguration => "Invalid Telescope configuration",
            Self::NotFound => "Telescope entry not found",
            Self::Database(_) => "Telescope database operation failed",
            Self::Hostname(_) => "Cannot read the host name",
            Self::Queue(_) => "Cannot dispatch Telescope pending updates",
        })
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Hostname(error) => Some(error),
            Self::Queue(error) => Some(error),
            _ => None,
        }
    }
}
impl From<openrust::database::DatabaseError> for Error {
    fn from(error: openrust::database::DatabaseError) -> Self {
        Self::Database(error)
    }
}
