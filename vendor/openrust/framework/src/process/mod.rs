//! Running and testing operating-system processes.

mod child;

#[cfg(unix)]
pub(crate) use child::{ChildGroup, inherit_queue_process_group};
mod facade;
mod factory;
mod invoked_process;
mod pending_process;
mod pool;
mod process_result;

pub use facade::Process;
pub(crate) use facade::ProcessServiceProvider;
pub use factory::{Factory, RecordedProcess};
pub use invoked_process::{InvokedProcess, OutputStream, Signal};
pub use pending_process::{Command, PendingProcess};
pub use pool::{InvokedProcessPool, Pool, ProcessPoolResults};
pub use process_result::ProcessResult;

use std::{fmt, io};

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    InvalidCommand,
    InvalidTimeout,
    Io(io::Error),
    TimedOut { idle: bool },
    Failed(Box<ProcessResult>),
    StrayProcess,
    SequenceExhausted,
    Unavailable(crate::contracts::container::BindingResolutionError),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommand => formatter.write_str("invalid process command"),
            Self::InvalidTimeout => formatter.write_str("invalid process timeout"),
            Self::Io(error) => write!(formatter, "process I/O failed: {error}"),
            Self::TimedOut { idle: true } => {
                formatter.write_str("process exceeded its idle timeout")
            }
            Self::TimedOut { idle: false } => formatter.write_str("process exceeded its timeout"),
            Self::Failed(result) => write!(
                formatter,
                "process exited with status {}",
                result.exitCode()
            ),
            Self::SequenceExhausted => formatter.write_str("process fake sequence is exhausted"),
            Self::StrayProcess => formatter.write_str("process has no matching fake"),
            Self::Unavailable(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
