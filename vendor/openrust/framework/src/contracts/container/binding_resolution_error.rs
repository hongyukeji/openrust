use std::{error::Error, fmt};

/// A failure while resolving a service from the container.
#[derive(Debug)]
#[non_exhaustive]
pub enum BindingResolutionError {
    NotBound {
        service: &'static str,
    },
    CircularDependency {
        services: Vec<&'static str>,
    },
    FactoryFailed {
        service: &'static str,
        source: Box<dyn Error + Send + Sync>,
    },
}

impl fmt::Display for BindingResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBound { service } => write!(f, "service {service} is not bound"),
            Self::CircularDependency { services } => {
                write!(f, "circular service dependency: {}", services.join(" -> "))
            }
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
