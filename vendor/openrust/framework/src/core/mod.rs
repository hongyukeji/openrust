#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod execution;
#[cfg(feature = "otel")]
pub(crate) mod otel;
pub mod rate_limit;
pub mod readiness;

pub use crate::config::NoConfig;
pub use error::{
    ApiResponse, Error, PageMeta, ResponseContext, Result, ValidationErrors, with_response_context,
};
pub(crate) use execution::CancelOnDrop;
pub use execution::{Cancellation, ExecutionContext, ExecutionMetadata, with_execution_context};

pub use rate_limit::{
    RateLimitBucket, RateLimitFuture, RateLimitStatus, RateLimitStore, RateLimiter,
};
pub use readiness::Readiness;
