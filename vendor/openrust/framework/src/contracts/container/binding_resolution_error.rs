use std::{error::Error, fmt};

use crate::container::ServiceKey;

/// A failure while resolving a service from the container.
#[derive(Debug)]
#[non_exhaustive]
pub enum BindingResolutionError {
    FacadeNotSet,
    NotBound {
        service: ServiceKey,
    },
    CircularDependency {
        services: Vec<ServiceKey>,
    },
    AliasToSelf {
        service: ServiceKey,
    },
    CircularAlias {
        services: Vec<ServiceKey>,
    },
    TypeMismatch {
        service: ServiceKey,
        expected: &'static str,
        actual: &'static str,
    },
    FactoryFailed {
        service: &'static str,
        source: Box<dyn Error + Send + Sync>,
    },
}

impl fmt::Display for BindingResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FacadeNotSet => f.write_str("a facade application has not been set"),
            Self::NotBound { service } => write!(f, "service {service} is not bound"),
            Self::CircularDependency { services } => {
                write!(
                    f,
                    "circular service dependency: {}",
                    services
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" -> ")
                )
            }
            Self::AliasToSelf { service } => write!(f, "[{service}] is aliased to itself"),
            Self::CircularAlias { services } => write!(
                f,
                "circular service alias: {}",
                services
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            Self::TypeMismatch {
                service,
                expected,
                actual,
            } => write!(f, "service {service} returns {actual}, expected {expected}"),
            Self::FactoryFailed { service, .. } => write!(f, "factory for {service} failed"),
        }
    }
}

impl Error for BindingResolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FactoryFailed { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
