use std::{error::Error, fmt, sync::Arc};

use crate::container::ServiceKey;

/// A failure while resolving a service from the container.
#[derive(Debug)]
#[non_exhaustive]
pub enum BindingResolutionError {
    MethodNotFound {
        service: ServiceKey,
        method: Arc<str>,
    },
    MethodBindingNotFound {
        service: ServiceKey,
        method: Arc<str>,
    },
    InvalidMethod {
        service: ServiceKey,
        method: Arc<str>,
        reason: &'static str,
    },
    MethodContractMismatch {
        service: ServiceKey,
        method: Arc<str>,
        expected: &'static str,
        actual: &'static str,
    },
    InvalidArgumentShape {
        index: usize,
    },
    InvalidConstructor {
        service: ServiceKey,
        reason: &'static str,
    },
    UnresolvableParameter {
        parameter: &'static str,
        service: Option<ServiceKey>,
    },
    InvalidArgumentSlot {
        index: usize,
    },
    ParameterTypeMismatch {
        expected: &'static str,
        actual: Option<&'static str>,
    },
    FacadeNotSet,
    ScopeClosed,
    AsyncResolutionRequired {
        service: ServiceKey,
    },
    DeferredServiceRequiresAsync {
        service: ServiceKey,
    },
    DeferredProviderFailed {
        provider: &'static str,
        source: Arc<dyn Error + Send + Sync>,
    },
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
            Self::MethodNotFound { service, method } => {
                write!(f, "method {service}@{method} is not declared")
            }
            Self::MethodBindingNotFound { service, method } => {
                write!(f, "method {service}@{method} has no binding")
            }
            Self::InvalidMethod {
                service,
                method,
                reason,
            } => write!(f, "invalid method {service}@{method}: {reason}"),
            Self::MethodContractMismatch {
                service,
                method,
                expected,
                actual,
            } => write!(
                f,
                "method {service}@{method} has contract {actual}, expected {expected}"
            ),
            Self::InvalidArgumentShape { index } => {
                write!(f, "constructor argument slot {index} has a different shape")
            }
            Self::InvalidConstructor { service, reason } => {
                write!(f, "invalid constructor for {service}: {reason}")
            }
            Self::UnresolvableParameter { parameter, service } => {
                write!(f, "constructor parameter {parameter} is unresolvable")?;
                if let Some(service) = service {
                    write!(f, " while building {service}")?;
                }
                Ok(())
            }
            Self::InvalidArgumentSlot { index } => {
                write!(f, "constructor argument slot {index} does not exist")
            }
            Self::ParameterTypeMismatch { expected, actual } => write!(
                f,
                "parameter returns {}, expected {expected}",
                actual.unwrap_or("null")
            ),
            Self::DeferredServiceRequiresAsync { service } => {
                write!(
                    f,
                    "service {service} requires asynchronous provider initialization; use getAsync or makeAsync"
                )
            }
            Self::AsyncResolutionRequired { service } => {
                write!(f, "service {service} requires asynchronous resolution")
            }
            Self::ScopeClosed => f.write_str("the service lifecycle has closed"),
            Self::FacadeNotSet => f.write_str("a facade application has not been set"),
            Self::DeferredProviderFailed { provider, .. } => {
                write!(f, "deferred provider {provider} failed")
            }
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
            Self::DeferredProviderFailed { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
