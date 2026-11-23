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

    fn getFacadeAccessor() -> ServiceKey {
        "cache".into()
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Cache {
    /// Registers a creator on the current application's cache manager.
    pub fn extend(
        driver: impl Into<String>,
        factory: impl Fn(&CacheManager, &serde_json::Value) -> CacheResult<Repository>
        + Send
        + Sync
        + 'static,
    ) -> CacheResult<()> {
        Self::getFacadeRoot()?.extend(driver, factory)?;
        Ok(())
    }

    pub fn build(config: serde_json::Value) -> CacheResult<Repository> {
        Self::getFacadeRoot()?.build(config)
    }

    pub fn forgetDriver(name: Option<&str>) -> CacheResult<()> {
        Self::getFacadeRoot()?.forgetDriver(name)?;
        Ok(())
    }

    pub fn purge(name: Option<&str>) -> CacheResult<()> {
        Self::getFacadeRoot()?.purge(name)
    }

    pub fn driver(name: Option<&str>) -> CacheResult<Arc<Repository>> {
        Self::store(name)
    }

    pub fn store(name: Option<&str>) -> CacheResult<Arc<Repository>> {
        Self::getFacadeRoot()?.store(name)
    }

    /// Memoizes reads of the default store or a named store for this lifecycle.
    pub fn memo(name: Option<&str>) -> CacheResult<Arc<Repository>> {
        Self::getFacadeRoot()?.memo(name)
    }

    pub fn tags(names: &[&str]) -> CacheResult<TaggedCache> {
        Self::store(None)?.tags(names)
    }

    pub fn getName() -> CacheResult<Option<String>> {
        Ok(Self::store(None)?.getName().map(str::to_owned))
    }

    pub fn getStore() -> CacheResult<Arc<dyn Store>> {
        Ok(Self::store(None)?.getStore())
    }

    pub fn getEventDispatcher() -> CacheResult<Option<Arc<dyn Dispatcher>>> {
        Ok(Self::store(None)?.getEventDispatcher())
    }

    pub fn getDefaultCacheTime() -> CacheResult<Option<i64>> {
        Ok(Self::store(None)?.getDefaultCacheTime())
    }

    pub async fn putWithDefault(key: &str, value: &Value) -> CacheResult<bool> {
        Self::store(None)?.putWithDefault(key, value).await
    }

    pub fn supportsTags() -> CacheResult<bool> {
        Ok(Self::store(None)?.supportsTags())
    }

    pub async fn many(keys: &[&str]) -> CacheResult<Vec<Option<Value>>> {
        Self::store(None)?.many(keys).await
    }

    pub async fn putMany(values: &[(String, Value)], ttl: Option<Ttl>) -> CacheResult<bool> {
        Self::store(None)?.putMany(values, ttl).await
    }

    pub async fn get(key: &str) -> CacheResult<Option<Value>> {
        Self::store(None)?.get(key).await
    }

    pub async fn getOr(key: &str, default: Value) -> CacheResult<Value> {
        Self::store(None)?.getOr(key, default).await
    }

    pub async fn getOrElse<F>(key: &str, default: impl FnOnce() -> F) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        Self::store(None)?.getOrElse(key, default).await
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

    pub async fn pullOr(key: &str, default: Value) -> CacheResult<Value> {
        Self::store(None)?.pullOr(key, default).await
    }

    pub async fn pullOrElse<F>(key: &str, default: impl FnOnce() -> F) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        Self::store(None)?.pullOrElse(key, default).await
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

    pub async fn rememberWithWarmth(
        key: &str,
        ttl: Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<(Value, bool)> {
        Self::store(None)?
            .rememberWithWarmth(key, ttl, resolve)
            .await
    }

    pub async fn rememberWithWarmthFor(
        key: &str,
        ttl: impl FnOnce(&Value) -> Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<(Value, bool)> {
        Self::store(None)?
            .rememberWithWarmthFor(key, ttl, resolve)
            .await
    }

    pub async fn rememberForever(
        key: &str,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        Self::store(None)?.rememberForever(key, resolve).await
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

    pub async fn flexibleWith<F>(
        key: &str,
        ttl: [Ttl; 2],
        resolve: impl FnOnce() -> F + Send + 'static,
        options: FlexibleOptions,
    ) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>> + Send + 'static,
    {
        Self::store(None)?
            .flexibleWith(key, ttl, resolve, options)
            .await
    }

    pub fn supportsFlushingLocks() -> CacheResult<bool> {
        Ok(Self::store(None)?.supportsFlushingLocks())
    }

    pub async fn flushLocks() -> CacheResult<bool> {
        Self::store(None)?.flushLocks().await
    }

    pub async fn flush() -> CacheResult<bool> {
        Self::store(None)?.flush().await
    }

    /// Builds a concurrency limiter on the default cache store.
    pub fn funnel(name: impl Into<String>) -> CacheResult<ConcurrencyLimiterBuilder> {
        Self::store(None)?.funnel(name)
    }

    /// Runs a single callback under an exclusive lock, waiting up to ten seconds.
    pub async fn withoutOverlapping<T, F>(key: &str, callback: impl FnOnce() -> F) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        Self::store(None)?.withoutOverlapping(key, callback).await
    }

    /// Uses explicit lease and wait durations in seconds, with an optional owner token.
    pub async fn withoutOverlappingWith<T, F>(
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
            .withoutOverlappingWith(key, callback, lock_for, wait_for, owner)
            .await
    }

    pub fn lock(name: &str, lease: Option<Duration>) -> CacheResult<Box<dyn Lock>> {
        Self::store(None)?.lock(name, lease, None)
    }

    pub fn restoreLock(name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        Self::store(None)?.restore_lock(name, owner)
    }
}
