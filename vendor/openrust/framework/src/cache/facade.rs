use std::{future::Future, sync::Arc, time::Duration};

use crate::{
    container::ServiceKey,
    contracts::cache::{Lock, LockProvider, Repository as _},
    support::facades::Facade,
};

use super::{CacheError, CacheManager, CacheResult, Repository, Ttl, Value};

/// Resolves cache operations for the current application.
pub struct Cache;

impl Facade for Cache {
    type Service = CacheManager;

    fn getFacadeAccessor() -> ServiceKey {
        "cache".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Cache facade API")]
impl Cache {
    pub fn store(name: Option<&str>) -> CacheResult<Arc<Repository>> {
        Self::getFacadeRoot()
            .map_err(CacheError::Unavailable)?
            .store(name)
    }

    pub async fn get(key: &str) -> CacheResult<Option<Value>> {
        Self::store(None)?.get(key).await
    }

    pub async fn put(key: &str, value: &Value, ttl: Option<Ttl>) -> CacheResult<bool> {
        Self::store(None)?.put(key, value, ttl).await
    }

    pub async fn add(key: &str, value: &Value, ttl: Option<Ttl>) -> CacheResult<bool> {
        Self::store(None)?.add(key, value, ttl).await
    }

    pub async fn forever(key: &str, value: &Value) -> CacheResult<bool> {
        Self::store(None)?.forever(key, value).await
    }

    pub async fn touch(key: &str, ttl: Ttl) -> CacheResult<bool> {
        Self::store(None)?.touch(key, ttl).await
    }

    pub async fn has(key: &str) -> CacheResult<bool> {
        Self::store(None)?.has(key).await
    }

    pub async fn missing(key: &str) -> CacheResult<bool> {
        Self::store(None)?.missing(key).await
    }

    pub async fn pull(key: &str) -> CacheResult<Option<Value>> {
        Self::store(None)?.pull(key).await
    }

    pub async fn forget(key: &str) -> CacheResult<bool> {
        Self::store(None)?.forget(key).await
    }

    pub async fn increment(key: &str, amount: i64) -> CacheResult<i64> {
        Self::store(None)?.increment(key, amount).await
    }

    pub async fn decrement(key: &str, amount: i64) -> CacheResult<i64> {
        Self::store(None)?.decrement(key, amount).await
    }

    pub async fn remember(
        key: &str,
        ttl: Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        Self::store(None)?.remember(key, ttl, resolve).await
    }

    pub async fn flush() -> CacheResult<bool> {
        Self::store(None)?.flush().await
    }

    pub fn lock(name: &str, lease: Option<Duration>) -> CacheResult<Box<dyn Lock>> {
        Self::store(None)?.lock(name, lease, None)
    }

    pub fn restoreLock(name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        Self::store(None)?.restore_lock(name, owner)
    }
}
