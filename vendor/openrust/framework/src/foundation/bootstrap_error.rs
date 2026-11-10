use std::{any::TypeId, error::Error, fmt, io, path::PathBuf};

use crate::{config::ConfigError, console::ConsoleError, routing::RoutingError};

/// A failure while creating or booting the application.
#[derive(Debug)]
#[non_exhaustive]
pub enum BootstrapError {
    ExecutablePathUnavailable {
        source: io::Error,
    },
    InvalidBasePath {
        path: PathBuf,
        source: io::Error,
    },
    ContainerFailed {
        source: crate::contracts::container::BindingResolutionError,
    },
    ConfigurationFailed {
        source: ConfigError,
    },
    RoutingFailed {
        source: RoutingError,
    },
    CommandRegistrationFailed {
        source: ConsoleError,
    },
    ProviderRegistrationFailed {
        provider: TypeId,
        name: &'static str,
        source: Box<dyn Error + Send + Sync>,
    },
    ProviderBootFailed {
        provider: TypeId,
        name: &'static str,
        source: Box<dyn Error + Send + Sync>,
    },
    PreviousBootFailed,
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExecutablePathUnavailable { source } => {
                write!(
                    f,
                    "cannot determine the application executable directory: {source}"
                )
            }
            Self::InvalidBasePath { path, source } => {
                write!(f, "invalid application root {}: {source}", path.display())
            }
            Self::ContainerFailed { .. } => f.write_str("service container failed"),
            Self::ConfigurationFailed { source } => write!(f, "configuration failed: {source}"),
            Self::RoutingFailed { source } => write!(f, "route registration failed: {source}"),
            Self::CommandRegistrationFailed { source } => {
                write!(f, "command registration failed: {source}")
            }
            Self::ProviderRegistrationFailed { name, .. } => {
                write!(f, "provider {name} failed during register")
            }
            Self::ProviderBootFailed { name, .. } => {
                write!(f, "provider {name} failed during boot")
            }
            Self::PreviousBootFailed => f.write_str("application startup failed previously"),
        }
    }
}

impl Error for BootstrapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ExecutablePathUnavailable { source } => Some(source),
            Self::InvalidBasePath { source, .. } => Some(source),
            Self::ContainerFailed { source } => Some(source),
            Self::ConfigurationFailed { source } => Some(source),
            Self::RoutingFailed { source } => Some(source),
            Self::CommandRegistrationFailed { source } => Some(source),
            Self::ProviderRegistrationFailed { source, .. }
            | Self::ProviderBootFailed { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
