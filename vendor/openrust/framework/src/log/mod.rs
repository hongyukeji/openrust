//! Application log channels and structured records.

pub mod context;
pub mod events;
mod log_manager;
mod log_service_provider;
mod logger;

pub use log_manager::LogManager;
pub use log_service_provider::LogServiceProvider;
pub use logger::Logger;

/// Severity levels in the application logging contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Emergency,
    Alert,
    Critical,
    Error,
    Warning,
    Notice,
    Info,
    Debug,
}

/// Structured values attached to a log record.
pub type LogContext = serde_json::Map<String, serde_json::Value>;

pub type LogResult<T> = Result<T, LogError>;

#[derive(Debug)]
#[non_exhaustive]
pub enum LogError {
    UnknownChannel(String),
    InvalidConfiguration,
    WriteFailed(Box<dyn std::error::Error + Send + Sync>),
}
