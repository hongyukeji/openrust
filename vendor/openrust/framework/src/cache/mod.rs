//! Cache repositories, stores, and atomic locks.

mod array_lock;
mod array_store;
mod cache_manager;
mod cache_service_provider;
mod database_lock;
mod database_store;
mod dynamo_db_lock;
mod dynamo_db_store;
mod failover_store;
mod file_lock;
mod file_store;
mod lock;
mod memcached_lock;
mod memcached_store;
mod null_store;
mod redis_lock;
mod redis_store;
mod repository;
mod session_store;
mod storage_store;

use std::{error::Error, future::Future, pin::Pin, time::SystemTime};

pub use array_lock::ArrayLock;
pub use array_store::ArrayStore;
pub use cache_manager::CacheManager;
pub use cache_service_provider::CacheServiceProvider;
pub use database_lock::DatabaseLock;
pub use database_store::DatabaseStore;
pub use dynamo_db_lock::DynamoDbLock;
pub use dynamo_db_store::DynamoDbStore;
pub use failover_store::FailoverStore;
pub use file_lock::FileLock;
pub use file_store::FileStore;
pub use lock::Lock;
pub use memcached_lock::MemcachedLock;
pub use memcached_store::MemcachedStore;
pub use null_store::NullStore;
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
    LocksUnsupported,
    LockTimeout,
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// A relative or absolute expiry for a cached value.
pub enum Ttl {
    Seconds(i64),
    Until(SystemTime),
}

/// A cached value, including binary payloads and null.
pub struct Value {
    _private: (),
}
