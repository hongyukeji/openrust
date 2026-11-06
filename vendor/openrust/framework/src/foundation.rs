#![doc = include_str!("foundation/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod application;
pub mod exceptions;
pub mod http;
pub mod lifecycle;
pub mod provider;
pub mod resources;

pub use application::{Application, ApplicationDefinition, ApplicationState, MissingServiceError};
pub use exceptions::{ExceptionHandler, ExceptionRequest, ReportDecision, ReportThrottle};
pub use lifecycle::{bootstrap, shutdown_signal};
pub use provider::{
    BootContext, ProviderBuilder, ProviderId, ProviderPreparation, ProviderRegistration,
    ProviderRuntime, RegisterContext,
};
