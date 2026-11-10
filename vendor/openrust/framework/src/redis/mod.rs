//! Named Redis connections and command transport.

pub mod connections;
pub mod connectors;
mod redis_manager;
mod redis_service_provider;

use std::{error::Error, future::Future, pin::Pin};

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
    UnknownConnection {
        name: String,
    },
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// A Redis response value that preserves its protocol type.
pub struct Value {
    _private: (),
}
