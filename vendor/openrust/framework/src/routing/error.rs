use std::{error::Error, fmt};

use crate::config::ConfigError;

/// An invalid route registration or middleware reference.
#[derive(Debug)]
#[non_exhaustive]
pub enum RoutingError {
    NoRegistrationScope,
    RegistrationBorrowed,
    ContainerFailed {
        source: crate::contracts::container::BindingResolutionError,
    },
    InvalidUri {
        uri: String,
    },
    EmptyStaticDirectory,
    ConflictingStorageRoute {
        uri: String,
    },
    StaticMountConfiguration {
        source: ConfigError,
    },
    MissingMethods,
    UnknownRouteParameter {
        name: String,
        uri: String,
    },
    InvalidConstraint {
        parameter: String,
        expression: String,
    },
    InvalidRoutePattern {
        uri: String,
        source: regex::Error,
    },
    UnknownMiddleware {
        name: String,
    },
    MiddlewareGroupCycle {
        name: String,
    },
}

impl fmt::Display for RoutingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContainerFailed { .. } => {
                f.write_str("service container failed during route registration")
            }
            Self::NoRegistrationScope => f.write_str("no active route registration scope"),
            Self::RegistrationBorrowed => {
                f.write_str("use the router provided to the route group callback")
            }
            Self::InvalidUri { uri } => write!(f, "invalid route URI {uri:?}"),
            Self::ConflictingStorageRoute { uri } => {
                write!(f, "multiple served disks share {uri:?}")
            }
            Self::EmptyStaticDirectory => f.write_str("a static mount requires a directory"),
            Self::StaticMountConfiguration { source } => source.fmt(f),
            Self::InvalidConstraint {
                parameter,
                expression,
            } => {
                write!(
                    f,
                    "invalid route constraint {expression:?} for parameter {parameter:?}"
                )
            }
            Self::InvalidRoutePattern { uri, source } => {
                write!(f, "invalid pattern for route {uri:?}: {source}")
            }
            Self::UnknownRouteParameter { name, uri } => {
                write!(f, "unknown parameter {name:?} on route {uri:?}")
            }
            Self::MissingMethods => f.write_str("a route requires at least one HTTP method"),
            Self::UnknownMiddleware { name } => write!(f, "unknown middleware {name:?}"),
            Self::MiddlewareGroupCycle { name } => write!(f, "middleware group cycle at {name:?}"),
        }
    }
}

impl Error for RoutingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ContainerFailed { source } => Some(source),
            Self::InvalidRoutePattern { source, .. } => Some(source),
            Self::StaticMountConfiguration { source } => Some(source),
            _ => None,
        }
    }
}
