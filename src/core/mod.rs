#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod app;
pub mod command;
pub mod container;
pub mod default_providers;
pub mod error;
pub mod exception_handler;
pub mod execution;
pub mod lifecycle;
pub mod module;
#[cfg(feature = "otel")]
pub(crate) mod otel;
pub mod plugin;
pub mod provider;
pub mod rate_limit;
pub mod readiness;
pub mod resources;

pub use crate::config::NoConfig;
pub use app::{App, AppState, Application, MissingServiceError};
pub use command::Command;
pub use default_providers::DefaultProviders;
pub use error::{
    ApiResponse, Error, PageMeta, ResponseContext, Result, ValidationErrors, with_response_context,
};
pub use exception_handler::{ExceptionHandler, ExceptionRequest, ReportDecision, ReportThrottle};
pub(crate) use execution::CancelOnDrop;
pub use execution::{Cancellation, ExecutionContext, ExecutionMetadata, with_execution_context};
pub use lifecycle::{bootstrap, shutdown_signal};
pub use module::{
    FileModuleActivator, Module, ModuleActivationSnapshot, ModuleActivator, ModuleConfigFiles,
    ModuleDescriptor, ModuleExecution, ModuleFile, ModuleLifecycleEvent, ModuleLifecyclePhase,
    ModuleLoadContext, ModuleManifest, ModuleRepository,
};
pub use plugin::BoxFuture;
pub use provider::{
    BootContext, ProviderBuilder, ProviderId, ProviderRuntime, RegisterContext, ServiceProvider,
};
pub use rate_limit::{
    RateLimitBucket, RateLimitFuture, RateLimitStatus, RateLimitStore, RateLimiter,
};
pub use readiness::Readiness;
