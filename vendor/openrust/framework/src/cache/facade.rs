use std::{future::Future, sync::Arc, time::Duration};

use crate::{
    container::ServiceKey,
    contracts::{
        cache::{Lock, LockProvider, Repository as _, Store},
        events::Dispatcher,
    },
    support::facades::Facade,
};

use super::{
    CacheManager, CacheResult, FlexibleOptions, Repository, TaggedCache, Ttl, Value,
    limiters::ConcurrencyLimiterBuilder,
};

/// Resolves cache operations for the current application.
pub struct Cache;

impl Facade for Cache {
    type Service = CacheManager;

    fn get_facade_accessor() -> ServiceKey {
        "cache".into()
    }
}

impl Cache {
    /// Registers a creator on the current application's cache manager.
    pub fn extend(
        driver: impl Into<String>,
        factory: impl Fn(&CacheManager, &serde_json::Value) -> CacheResult<Repository>
        + Send
        + Sync
        + 'static,
    ) -> CacheResult<()> {
        Self::get_facade_root()?.extend(driver, factory)?;
        Ok(())
    }

    pub fn build(config: serde_json::Value) -> CacheResult<Repository> {
        Self::get_facade_root()?.build(config)
    }

    pub fn forget_driver(name: Option<&str>) -> CacheResult<()> {
        Self::get_facade_root()?.forget_driver(name)?;
        Ok(())
    }

    pub fn purge(name: Option<&str>) -> CacheResult<()> {
        Self::get_facade_root()?.purge(name)
    }

    pub fn driver(name: Option<&str>) -> CacheResult<Arc<Repository>> {
        Self::store(name)
    }

    pub fn store(name: Option<&str>) -> CacheResult<Arc<Repository>> {
        Self::get_facade_root()?.store(name)
    }

    /// Memoizes reads of the default store or a named store for this lifecycle.
    pub fn memo(name: Option<&str>) -> CacheResult<Arc<Repository>> {
        Self::get_facade_root()?.memo(name)
    }

    pub fn tags(names: &[&str]) -> CacheResult<TaggedCache> {
        Self::store(None)?.tags(names)
    }

    pub fn get_name() -> CacheResult<Option<String>> {
        Ok(Self::store(None)?.get_name().map(str::to_owned))
    }

    pub fn get_store() -> CacheResult<Arc<dyn Store>> {
        Ok(Self::store(None)?.get_store())
    }

    pub fn get_event_dispatcher() -> CacheResult<Option<Arc<dyn Dispatcher>>> {
        Ok(Self::store(None)?.get_event_dispatcher())
    }

    pub fn get_default_cache_time() -> CacheResult<Option<i64>> {
        Ok(Self::store(None)?.get_default_cache_time())
    }

    pub async fn put_with_default(key: &str, value: &Value) -> CacheResult<bool> {
        Self::store(None)?.put_with_default(key, value).await
    }

    pub fn supports_tags() -> CacheResult<bool> {
        Ok(Self::store(None)?.supports_tags())
    }

    pub async fn many(keys: &[&str]) -> CacheResult<Vec<Option<Value>>> {
        Self::store(None)?.many(keys).await
    }

    pub async fn put_many(values: &[(String, Value)], ttl: Option<Ttl>) -> CacheResult<bool> {
        Self::store(None)?.put_many(values, ttl).await
    }

    pub async fn get(key: &str) -> CacheResult<Option<Value>> {
        Self::store(None)?.get(key).await
    }

    pub async fn get_or(key: &str, default: Value) -> CacheResult<Value> {
        Self::store(None)?.get_or(key, default).await
    }

    pub async fn get_or_else<F>(key: &str, default: impl FnOnce() -> F) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        Self::store(None)?.get_or_else(key, default).await
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

    pub async fn pull_or(key: &str, default: Value) -> CacheResult<Value> {
        Self::store(None)?.pull_or(key, default).await
    }

    pub async fn pull_or_else<F>(key: &str, default: impl FnOnce() -> F) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        Self::store(None)?.pull_or_else(key, default).await
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

    pub async fn remember_with_warmth(
        key: &str,
        ttl: Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<(Value, bool)> {
        Self::store(None)?
            .remember_with_warmth(key, ttl, resolve)
            .await
    }

    pub async fn remember_with_warmth_for(
        key: &str,
        ttl: impl FnOnce(&Value) -> Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<(Value, bool)> {
        Self::store(None)?
            .remember_with_warmth_for(key, ttl, resolve)
            .await
    }

    pub async fn remember_forever(
        key: &str,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        Self::store(None)?.remember_forever(key, resolve).await
    }

    pub async fn sear(
        key: &str,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        Self::store(None)?.sear(key, resolve).await
    }

    pub async fn flexible<F>(
        key: &str,
        ttl: [i64; 2],
        resolve: impl FnOnce() -> F + Send + 'static,
    ) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>> + Send + 'static,
    {
        Self::store(None)?.flexible(key, ttl, resolve).await
    }

    pub async fn flexible_with<F>(
        key: &str,
        ttl: [Ttl; 2],
        resolve: impl FnOnce() -> F + Send + 'static,
        options: FlexibleOptions,
    ) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>> + Send + 'static,
    {
        Self::store(None)?
            .flexible_with(key, ttl, resolve, options)
            .await
    }

    pub fn supports_flushing_locks() -> CacheResult<bool> {
        Ok(Self::store(None)?.supports_flushing_locks())
    }

    pub async fn flush_locks() -> CacheResult<bool> {
        Self::store(None)?.flush_locks().await
    }

    pub async fn flush() -> CacheResult<bool> {
        Self::store(None)?.flush().await
    }

    /// Builds a concurrency limiter on the default cache store.
    pub fn funnel(name: impl Into<String>) -> CacheResult<ConcurrencyLimiterBuilder> {
        Self::store(None)?.funnel(name)
    }

    /// Runs a single callback under an exclusive lock, waiting up to ten seconds.
    pub async fn without_overlapping<T, F>(
        key: &str,
        callback: impl FnOnce() -> F,
    ) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        Self::store(None)?.without_overlapping(key, callback).await
    }

    /// Uses explicit lease and wait durations in seconds, with an optional owner token.
    pub async fn without_overlapping_with<T, F>(
        key: &str,
        callback: impl FnOnce() -> F,
        lock_for: u64,
        wait_for: u64,
        owner: Option<&str>,
    ) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        Self::store(None)?
            .without_overlapping_with(key, callback, lock_for, wait_for, owner)
            .await
    }

    pub fn lock(name: &str, lease: Option<Duration>) -> CacheResult<Box<dyn Lock>> {
        Self::store(None)?.lock(name, lease, None)
    }

    pub fn restore_lock(name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        Self::store(None)?.restore_lock(name, owner)
    }
}
