//! Named Redis connections and command transport.

pub mod connections;
pub mod connectors;
pub mod events;
mod facade;
pub mod limiters;
mod redis_manager;
mod redis_service_provider;

use std::{error::Error, future::Future, pin::Pin};

pub use ::redis::Value;
pub use facade::Redis;
pub use redis_manager::RedisManager;
pub use redis_service_provider::RedisServiceProvider;

/// An asynchronous Redis operation.
pub type RedisFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a Redis operation.
pub type RedisResult<T> = Result<T, RedisError>;

/// A Redis configuration, connection, or command failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum RedisError {
    InvalidConfiguration,
    UnsupportedClusterOperation(&'static str),
    Unavailable(crate::contracts::container::BindingResolutionError),
    UnknownConnection {
        name: String,
    },
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
}

impl RedisError {
    pub(crate) fn is_cluster_redirect(&self) -> bool {
        matches!(self.source().and_then(|cause| cause.downcast_ref::<::redis::RedisError>()), Some(error) if error.redirect_node().is_some())
    }
}

impl std::fmt::Display for RedisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration => formatter.write_str("Invalid Redis configuration."),
            Self::UnsupportedClusterOperation(operation) => {
                write!(
                    formatter,
                    "Redis Cluster does not support {operation} through this connection."
                )
            }
            Self::Unavailable(_) => formatter.write_str("Redis manager is unavailable."),
            Self::UnknownConnection { name } => {
                write!(formatter, "Unknown Redis connection '{name}'.")
            }
            Self::Backend { .. } => formatter.write_str("Redis command or connection failed."),
        }
    }
}

impl Error for RedisError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unavailable(source) => Some(source),
            Self::Backend { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

pub(crate) fn backend(source: impl Error + Send + Sync + 'static) -> RedisError {
    RedisError::Backend {
        source: Box::new(source),
    }
}

/// Native Redis command batching, shared by queues and optional extensions.
pub use ::redis::Pipeline;
