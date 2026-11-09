#![doc = include_str!("foundation/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod application;
pub mod events;
pub mod exceptions;
mod extensions;
pub mod http;
pub mod lifecycle;
mod lifecycle_duration;
pub mod provider;
pub mod resources;
mod termination;

pub use application::{Application, ApplicationDefinition, ApplicationState, MissingServiceError};
pub use exceptions::{ExceptionHandler, ExceptionRequest, ReportDecision, ReportThrottle};
pub use extensions::{ExtensionMut, ExtensionRef};
pub use lifecycle::{bootstrap, shutdown_signal};
pub(crate) use lifecycle_duration::EntryStarted;
pub use lifecycle_duration::{CommandLifecycle, LifecycleThreshold, RequestLifecycle};
pub use provider::{
    BootContext, DeferrableProvider, PackageProvider, ProviderBuilder, ProviderId,
    ProviderPreparation, ProviderRegistration, ProviderRuntime, RegisterContext,
    RegisteredProvider,
};
