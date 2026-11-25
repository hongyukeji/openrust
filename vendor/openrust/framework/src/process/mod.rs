//! Running and testing operating-system processes.

#[cfg(windows)]
pub(crate) mod windows_job;
#[cfg(windows)]
mod windows_shutdown;
#[cfg(windows)]
pub use windows_shutdown::ParentShutdown;

mod child;

pub(crate) use child::ChildGroup;
#[cfg(unix)]
pub(crate) use child::inherit_queue_process_group;
mod facade;
mod factory;
mod fake_description;
mod invoked_process;
mod pending_process;
mod pool;
mod process_result;
#[cfg(windows)]
pub(crate) mod windows_background;

pub use facade::Process;
pub(crate) use facade::ProcessServiceProvider;
pub use factory::{Factory, ProcessAssertion, RecordedProcess};
pub use fake_description::FakeProcessDescription;
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
    TimedOut {
        idle: bool,
        result: Box<ProcessResult>,
    },
    TtyUnavailable,
    OutputUnavailable,
    Failed(Box<ProcessResult>),
    StrayProcess,
    SequenceExhausted,
    Unavailable(crate::contracts::container::BindingResolutionError),
}

impl Error {
    pub fn result(&self) -> Option<&ProcessResult> {
        match self {
            Self::TimedOut { result, .. } | Self::Failed(result) => Some(result),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommand => formatter.write_str("invalid process command"),
            Self::InvalidTimeout => formatter.write_str("invalid process timeout"),
            Self::Io(error) => write!(formatter, "process I/O failed: {error}"),
            Self::TimedOut { idle: true, .. } => {
                formatter.write_str("process exceeded its idle timeout")
            }
            Self::TimedOut { idle: false, .. } => {
                formatter.write_str("process exceeded its timeout")
            }
            Self::TtyUnavailable => formatter.write_str("process requires a controlling terminal"),
            Self::OutputUnavailable => formatter.write_str("process output is not captured"),
            Self::Failed(result) => write!(
                formatter,
                "process exited with status {}",
                result.exit_code()
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

impl From<crate::contracts::container::BindingResolutionError> for Error {
    fn from(source: crate::contracts::container::BindingResolutionError) -> Self {
        Self::Unavailable(source)
    }
}
