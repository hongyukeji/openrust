//! Application tasks executed locally or in isolated Artisan processes.

mod deferred;
mod facade;
mod invoke_serialized_closure;
mod process_driver;
mod protocol;
mod registry;

pub use deferred::DeferredCallback;
pub(crate) use deferred::DeferredCallbacks;
pub(crate) use facade::ConcurrencyServiceProvider;
pub use facade::{Concurrency, Driver, Manager};
pub(crate) use invoke_serialized_closure::register;
pub use process_driver::ProcessDriver;
pub use registry::{Registry, Task};

use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    InvalidTask,
    DuplicateTask(String),
    UnknownTask(String),
    TaskFailed { task: String, message: String },
    UnknownDriver(String),
    ChildFailed { task: String, exit_code: i32 },
    Process(crate::process::Error),
    InvalidEnvelope,
    PayloadTooLarge,
    Encoding(serde_json::Error),
    Io(std::io::Error),
    Unavailable(crate::contracts::container::BindingResolutionError),
    OutsideRequest,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTask => formatter.write_str("invalid concurrency task name"),
            Self::DuplicateTask(name) => {
                write!(formatter, "concurrency task {name} is already registered")
            }
            Self::UnknownTask(name) => {
                write!(formatter, "concurrency task {name} is not registered")
            }
            Self::TaskFailed { task, message } => {
                write!(formatter, "concurrency task {task} failed: {message}")
            }
            Self::UnknownDriver(name) => write!(formatter, "unknown concurrency driver {name}"),
            Self::ChildFailed { task, exit_code } => write!(
                formatter,
                "concurrency task {task} exited with code {exit_code}"
            ),
            Self::Process(error) => error.fmt(formatter),
            Self::InvalidEnvelope => formatter.write_str("invalid concurrency result envelope"),
            Self::PayloadTooLarge => {
                formatter.write_str("concurrency input exceeds the worker payload limit")
            }
            Self::Encoding(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Unavailable(error) => error.fmt(formatter),
            Self::OutsideRequest => {
                formatter.write_str("deferred concurrency requires an active HTTP request")
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
