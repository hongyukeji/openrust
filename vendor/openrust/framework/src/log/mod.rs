//! Application log channels and structured records.

macro_rules! log_methods {
    ($($receiver:ident)?) => {
        log_methods!(@method emergency, Emergency $(, $receiver)?);
        log_methods!(@method alert, Alert $(, $receiver)?);
        log_methods!(@method critical, Critical $(, $receiver)?);
        log_methods!(@method error, Error $(, $receiver)?);
        log_methods!(@method warning, Warning $(, $receiver)?);
        log_methods!(@method notice, Notice $(, $receiver)?);
        log_methods!(@method info, Info $(, $receiver)?);
        log_methods!(@method debug, Debug $(, $receiver)?);
    };
    (@method $method:ident, $level:ident, $receiver:ident) => {
        pub fn $method(&$receiver, message: &str, context: &LogContext) -> LogResult<()> {
            $receiver.log(Level::$level, message, context)
        }
    };
    (@method $method:ident, $level:ident) => {
        pub fn $method(message: &str, context: &LogContext) -> LogResult<()> {
            Self::log(Level::$level, message, context)
        }
    };
}

mod facade;
mod rotating;
mod sink;
pub use facade::Log;

pub mod context;
pub mod events;
mod log_manager;
mod log_service_provider;
mod logger;

pub use log_manager::{LogManager, StackChannel};
pub use log_service_provider::LogServiceProvider;
pub use logger::Logger;

/// Severity levels in the application logging contract.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Level {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl Level {
    fn parse(value: &str) -> LogResult<Self> {
        match value {
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "notice" => Ok(Self::Notice),
            "warning" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            "critical" => Ok(Self::Critical),
            "alert" => Ok(Self::Alert),
            "emergency" => Ok(Self::Emergency),
            _ => Err(LogError::InvalidConfiguration),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Notice => "NOTICE",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Critical => "CRITICAL",
            Self::Alert => "ALERT",
            Self::Emergency => "EMERGENCY",
        }
    }
}

/// Structured values attached to a log record.
pub type LogContext = serde_json::Map<String, serde_json::Value>;

pub type LogResult<T> = Result<T, LogError>;

#[derive(Debug)]
#[non_exhaustive]
pub enum LogError {
    UnknownChannel(String),
    InvalidConfiguration,
    CyclicStack,
    FactoryFailed,
    Unavailable(crate::contracts::container::BindingResolutionError),
    WriteFailed(Box<dyn std::error::Error + Send + Sync>),
}

impl LogError {
    fn write(error: std::io::Error) -> Self {
        Self::WriteFailed(Box::new(error))
    }
}

impl std::fmt::Display for LogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::UnknownChannel(_) => "log channel is not defined",
            Self::InvalidConfiguration => "invalid log channel configuration",
            Self::FactoryFailed => "custom log channel factory failed",
            Self::CyclicStack => "cyclic or excessively deep log stack",
            Self::Unavailable(_) => "application log manager is unavailable",
            Self::WriteFailed(_) => "unable to write the log record",
        })
    }
}

impl std::error::Error for LogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            Self::WriteFailed(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
