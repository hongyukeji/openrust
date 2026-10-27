#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod app;
pub mod command;
pub mod error;
pub mod execution;
pub mod lifecycle;
pub mod plugin;
pub mod rate_limit;
pub mod readiness;

pub use crate::config::NoConfig;
pub use app::{App, AppBuilder, AppState, Application, MissingServiceError};
pub use command::Command;
pub use error::{
    ApiResponse, Error, PageMeta, ResponseContext, Result, ValidationErrors, with_response_context,
};
pub(crate) use execution::CancelOnDrop;
pub use execution::{Cancellation, ExecutionContext, ExecutionMetadata, with_execution_context};
pub use lifecycle::{bootstrap, bootstrap_with, finalize, shutdown_signal};
pub use plugin::{BoxFuture, Plugin};
pub use rate_limit::{
    RateLimitBucket, RateLimitFuture, RateLimitStatus, RateLimitStore, RateLimiter,
};
pub use readiness::Readiness;
