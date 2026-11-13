//! Cache repositories, stores, and atomic locks.

mod array_lock;
mod array_store;
mod cache_manager;
mod cache_service_provider;
pub(crate) mod console;
mod database_lock;
mod database_store;
mod dynamo_db_lock;
mod dynamo_db_store;
mod engine;
mod facade;
mod failover_store;
mod file_lock;
mod file_store;
mod lock;
mod memcached_lock;
mod memcached_store;
mod memoized_store;
mod null_store;
mod rate_limiter;
pub mod rate_limiting;
mod redis_lock;
mod redis_store;
mod repository;
mod session_store;
mod storage_store;
mod store;

use std::{error::Error, future::Future, pin::Pin, time::SystemTime};

pub use array_lock::ArrayLock;
pub use array_store::ArrayStore;
pub use cache_manager::CacheManager;
pub use cache_service_provider::CacheServiceProvider;
pub use database_lock::DatabaseLock;
pub use database_store::DatabaseStore;
pub use dynamo_db_lock::DynamoDbLock;
pub use dynamo_db_store::DynamoDbStore;
pub use facade::Cache;
pub use failover_store::FailoverStore;
pub use file_lock::FileLock;
pub use file_store::FileStore;
pub use lock::Lock;
pub use memcached_lock::MemcachedLock;
pub use memcached_store::MemcachedStore;
pub use null_store::NullStore;
pub use rate_limiter::RateLimiter;
pub use redis_lock::RedisLock;
pub use redis_store::RedisStore;
pub use repository::Repository;
pub use session_store::SessionStore;
pub use storage_store::StorageStore;

/// An asynchronous cache operation.
pub type CacheFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a cache operation.
pub type CacheResult<T> = Result<T, CacheError>;

/// A configuration, backend, or lock failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum CacheError {
    UnknownStore {
        name: String,
    },
    UnsupportedDriver,
    InvalidConfiguration,
    InvalidValue,
    MissingCounter,
    InvalidOwner,
    Unavailable(crate::contracts::container::BindingResolutionError),
    LocksUnsupported,
    LockTimeout,
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// A relative or absolute expiry for a cached value.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum Ttl {
    Seconds(i64),
    Until(SystemTime),
}

/// A cached value, including binary payloads and null.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Value {
    Json(serde_json::Value),
    Bytes(Vec<u8>),
}

impl From<serde_json::Value> for Value {
    fn from(value: serde_json::Value) -> Self {
        Self::Json(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Json(value.into())
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Json(value.into())
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Json(serde_json::Value::Null))
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Json(serde_json::Value::String(text)) => text.trim().parse().ok(),
            Self::Json(value) => value.as_i64(),
            Self::Bytes(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Json(value) => value.as_str(),
            Self::Bytes(_) => None,
        }
    }
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownStore { name } => write!(f, "Cache store [{name}] is not configured."),
            Self::UnsupportedDriver => f.write_str("The cache driver is unsupported."),
            Self::InvalidConfiguration => f.write_str("Invalid cache configuration."),
            Self::InvalidValue => f.write_str("Invalid cache value or expiration."),
            Self::MissingCounter => f.write_str("The cache counter does not exist."),
            Self::InvalidOwner => f.write_str("Invalid cache lock owner."),
            Self::Unavailable(_) => f.write_str("The application cache is unavailable."),
            Self::LocksUnsupported => f.write_str("The cache store does not support locks."),
            Self::LockTimeout => f.write_str("Timed out acquiring the cache lock."),
            Self::Backend { .. } => f.write_str("The cache backend operation failed."),
        }
    }
}

impl Error for CacheError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Backend { source } => Some(source.as_ref()),
            Self::Unavailable(source) => Some(source),
            _ => None,
        }
    }
}

pub(super) fn backend(source: impl Error + Send + Sync + 'static) -> CacheError {
    CacheError::Backend {
        source: Box::new(source),
    }
}
