use std::{
    future::Future,
    num::NonZeroU64,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

use crate::{
    contracts::{
        cache::{Lock, LockProvider, Repository as Contract, Store},
        events::{Dispatcher as EventDispatcher, Event},
    },
    events::EventContext,
};

use super::{
    CacheError, CacheFuture, CacheResult, TaggedCache, Ttl, Value, backend, events::*,
    limiters::ConcurrencyLimiterBuilder,
};

/// Applies application-level semantics to a cache store.
pub struct Repository {
    store: RwLock<Arc<dyn Store>>,
    name: Option<String>,
    default_cache_time: RwLock<Option<i64>>,
    dispatcher: RwLock<Option<Arc<dyn EventDispatcher>>>,
    tags: Vec<String>,
    pub(super) macros: Arc<super::macro_support::CacheMacros>,
}

impl Clone for Repository {
    fn clone(&self) -> Self {
        Self {
            store: RwLock::new(self.get_store()),
            name: self.name.clone(),
            default_cache_time: RwLock::new(self.get_default_cache_time()),
            dispatcher: RwLock::new(
                self.dispatcher
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            ),
            tags: self.tags.clone(),
            macros: self.macros.clone(),
        }
    }
}

impl Repository {
    /// Builds a concurrency limiter on this repository's store.
    pub fn funnel(&self, name: impl Into<String>) -> CacheResult<ConcurrencyLimiterBuilder> {
        let store = self.get_store();
        store.lock_provider().ok_or(CacheError::LocksUnsupported)?;
        Ok(ConcurrencyLimiterBuilder::new(store, name.into()))
    }

    /// Runs a single callback under an exclusive lock, waiting up to ten seconds.
    pub async fn without_overlapping<T, F>(
        &self,
        key: &str,
        callback: impl FnOnce() -> F,
    ) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        self.without_overlapping_with(key, callback, 0, 10, None)
            .await
    }

    /// Uses explicit lease and wait durations in seconds, with an optional owner token.
    pub async fn without_overlapping_with<T, F>(
        &self,
        key: &str,
        callback: impl FnOnce() -> F,
        lock_for: u64,
        wait_for: u64,
        owner: Option<&str>,
    ) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        self.lock(key, Some(Duration::from_secs(lock_for)), owner)?
            .block_with(Duration::from_secs(wait_for), callback)
            .await
    }

    pub fn tags(&self, names: &[&str]) -> CacheResult<TaggedCache> {
        let mut tagged = self.get_store().tags(names)?;
        tagged.repository.name.clone_from(&self.name);
        tagged
            .repository
            .set_default_cache_time(self.get_default_cache_time());
        let dispatcher = self
            .dispatcher
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if dispatcher.is_some() {
            *tagged
                .repository
                .dispatcher
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = dispatcher;
        }
        tagged.repository.macros = self.macros.clone();
        tagged.repository.tags = names.iter().map(|name| (*name).to_owned()).collect();
        Ok(tagged)
    }

    pub fn supports_tags(&self) -> bool {
        self.get_store().supports_tags()
    }

    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store: RwLock::new(store),
            name: None,
            default_cache_time: RwLock::new(Some(3600)),
            dispatcher: RwLock::new(None),
            tags: Vec::new(),
            macros: Arc::default(),
        }
    }

    pub(super) fn memoized_view(&self, store: Arc<dyn Store>) -> Self {
        Self::new(store).with_macros(self.macros.clone())
    }

    pub(super) async fn clear_store(&self, store: Arc<dyn Store>) -> CacheResult<bool> {
        let mut repository = self.clone();
        repository.set_store(store);
        repository.tags.clear();
        repository.flush().await
    }

    pub(super) fn with_name(mut self, name: Option<&str>) -> Self {
        self.name = name.map(str::to_owned);
        self
    }

    /// Returns the name assigned when the repository was constructed.
    pub fn get_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns a handle to this repository's storage backend.
    pub fn get_store(&self) -> Arc<dyn Store> {
        self.store
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// 替换当前仓储的后端；已取得的后端句柄与进行中的操作保持原有归属。
    pub fn set_store(&self, store: Arc<dyn Store>) -> &Self {
        let retired = {
            let mut current = self
                .store
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::replace(&mut *current, store)
        };
        drop(retired);
        self
    }

    /// Returns the current event dispatcher, if event delivery is configured.
    pub fn get_event_dispatcher(&self) -> Option<Arc<dyn EventDispatcher>> {
        self.dispatcher
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// 替换实际事件派发器；回调和旧派发器均在设置锁外释放。
    pub fn set_event_dispatcher(&self, dispatcher: Arc<dyn EventDispatcher>) {
        let retired = self
            .dispatcher
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(dispatcher);
        drop(retired);
    }

    /// Returns the lifetime used by `put_with_default`, in seconds.
    pub fn get_default_cache_time(&self) -> Option<i64> {
        *self
            .default_cache_time
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Sets the default lifetime. `None` writes values indefinitely.
    pub fn set_default_cache_time(&self, seconds: Option<i64>) -> &Self {
        *self
            .default_cache_time
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = seconds;
        self
    }

    /// Writes a value with this repository's default lifetime.
    pub async fn put_with_default(&self, key: &str, value: &Value) -> CacheResult<bool> {
        self.put(key, value, self.get_default_cache_time().map(Ttl::Seconds))
            .await
    }

    async fn emit<E: Event>(
        &self,
        event: impl FnOnce(Option<String>, Vec<String>) -> E,
    ) -> CacheResult<()> {
        if let Some(dispatcher) = self.get_event_dispatcher() {
            dispatcher
                .dispatch(
                    Arc::new(event(self.name.clone(), self.tags.clone())),
                    &mut EventContext::new(),
                )
                .await
                .map_err(backend)?;
        }
        Ok(())
    }

    async fn retrieved(&self, key: &str, value: Option<&Value>) -> CacheResult<()> {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            self.emit(|store_name, tags| CacheHit {
                store_name,
                key: key.into(),
                tags,
                value: value.clone(),
            })
            .await
        } else {
            self.emit(|store_name, tags| CacheMissed {
                store_name,
                key: key.into(),
                tags,
            })
            .await
        }
    }

    async fn written(
        &self,
        key: &str,
        value: &Value,
        seconds: Option<u64>,
        success: bool,
    ) -> CacheResult<()> {
        if success {
            self.emit(|store_name, tags| KeyWritten {
                store_name,
                key: key.into(),
                tags,
                value: value.clone(),
                seconds,
            })
            .await
        } else {
            self.emit(|store_name, tags| KeyWriteFailed {
                store_name,
                key: key.into(),
                tags,
                value: value.clone(),
                seconds,
            })
            .await
        }
    }

    async fn forgotten(&self, key: &str, success: bool) -> CacheResult<()> {
        if success {
            self.emit(|store_name, tags| KeyForgotten {
                store_name,
                key: key.into(),
                tags,
            })
            .await
        } else {
            self.emit(|store_name, tags| KeyForgetFailed {
                store_name,
                key: key.into(),
                tags,
            })
            .await
        }
    }

    pub async fn many(&self, keys: &[&str]) -> CacheResult<Vec<Option<Value>>> {
        self.emit(|store_name, tags| RetrievingManyKeys {
            store_name,
            tags,
            key: keys.first().copied().unwrap_or_default().into(),
            keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        })
        .await?;
        let values = self.get_store().many(keys).await?;
        for (key, value) in keys.iter().zip(&values) {
            self.retrieved(key, value.as_ref()).await?;
        }
        Ok(values)
    }

    pub async fn put_many(
        &self,
        values: &[(String, Value)],
        ttl: Option<Ttl>,
    ) -> CacheResult<bool> {
        match ttl {
            Some(ttl) => match seconds(ttl) {
                Some(seconds) => self.put_many_seconds(values, seconds).await,
                None => {
                    let keys: Vec<_> = values.iter().map(|(key, _)| key.as_str()).collect();
                    self.delete_multiple(&keys).await
                }
            },
            None => {
                let mut written = true;
                for (key, value) in values {
                    written &= self.forever(key, value).await?;
                }
                Ok(written)
            }
        }
    }

    pub(super) async fn put_seconds(
        &self,
        key: &str,
        value: &Value,
        seconds: Option<NonZeroU64>,
    ) -> CacheResult<bool> {
        self.emit(|store_name, tags| WritingKey {
            store_name,
            key: key.into(),
            tags,
            value: value.clone(),
            seconds: seconds.map(NonZeroU64::get),
        })
        .await?;
        let result = match seconds {
            Some(seconds) => self.get_store().put(key, value, seconds).await?,
            None => self.get_store().forever(key, value).await?,
        };
        self.written(key, value, seconds.map(NonZeroU64::get), result)
            .await?;
        Ok(result)
    }

    pub(super) async fn put_many_seconds(
        &self,
        entries: &[(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheResult<bool> {
        self.emit(|store_name, tags| WritingManyKeys {
            store_name,
            tags,
            key: entries
                .first()
                .map(|(key, _)| key.clone())
                .unwrap_or_default(),
            keys: entries.iter().map(|(key, _)| key.clone()).collect(),
            values: entries.iter().map(|(_, value)| value.clone()).collect(),
            seconds: Some(seconds.get()),
        })
        .await?;
        let result = self.get_store().put_many(entries, seconds).await?;
        for (key, value) in entries {
            self.written(key, value, Some(seconds.get()), result)
                .await?;
        }
        Ok(result)
    }

    pub async fn has(&self, key: &str) -> CacheResult<bool> {
        Ok(self.get(key).await?.is_some_and(|value| !value.is_null()))
    }

    pub async fn missing(&self, key: &str) -> CacheResult<bool> {
        Ok(!self.has(key).await?)
    }

    /// Reads a non-null value, returning the supplied default on a miss.
    pub async fn get_or(&self, key: &str, default: Value) -> CacheResult<Value> {
        self.get_or_else(key, || async { Ok(default) }).await
    }

    /// Resolves the default only when the key is missing or contains null.
    pub async fn get_or_else<F>(&self, key: &str, default: impl FnOnce() -> F) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        match self.get(key).await?.filter(|value| !value.is_null()) {
            Some(value) => Ok(value),
            None => default().await,
        }
    }

    /// Atomically removes the entry, returning a default if it was missing or null.
    pub async fn pull_or(&self, key: &str, default: Value) -> CacheResult<Value> {
        self.pull_or_else(key, || async { Ok(default) }).await
    }

    /// Atomically removes the entry before resolving the default on a miss.
    pub async fn pull_or_else<F>(
        &self,
        key: &str,
        default: impl FnOnce() -> F,
    ) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        match self.pull(key).await?.filter(|value| !value.is_null()) {
            Some(value) => Ok(value),
            None => default().await,
        }
    }

    pub async fn remember(
        &self,
        key: &str,
        ttl: Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        self.remember_with_warmth(key, ttl, resolve)
            .await
            .map(|(value, _)| value)
    }

    /// Returns the cached value or resolves it, with a flag indicating a non-null hit.
    pub async fn remember_with_warmth(
        &self,
        key: &str,
        ttl: Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<(Value, bool)> {
        self.remember_with_warmth_for(key, |_| ttl, resolve).await
    }

    /// Computes the expiry from the resolved value on a miss.
    pub async fn remember_with_warmth_for(
        &self,
        key: &str,
        ttl: impl FnOnce(&Value) -> Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<(Value, bool)> {
        if let Some(value) = self.get(key).await?.filter(|value| !value.is_null()) {
            return Ok((value, true));
        }
        let value = resolve.await?;
        self.put(key, &value, ttl(&value)).await?;
        Ok((value, false))
    }

    /// Resolves a missing value and stores it without an expiry.
    pub async fn remember_forever(
        &self,
        key: &str,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        self.remember(key, None, resolve).await
    }

    /// Alias for `remember_forever`.
    pub async fn sear(
        &self,
        key: &str,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        self.remember_forever(key, resolve).await
    }

    pub fn supports_flushing_locks(&self) -> bool {
        self.get_store().lock_flusher().is_some()
    }

    pub async fn flush_locks(&self) -> CacheResult<bool> {
        let backend = self.get_store();
        let store = backend
            .lock_flusher()
            .ok_or(CacheError::LockFlushUnsupported)?;
        self.emit(|store_name, _| CacheLocksFlushing { store_name })
            .await?;
        let result = store.flush_locks().await?;
        if result {
            self.emit(|store_name, _| CacheLocksFlushed { store_name })
                .await?;
        } else {
            self.emit(|store_name, _| CacheLocksFlushFailed { store_name })
                .await?;
        }
        Ok(result)
    }

    pub async fn flush(&self) -> CacheResult<bool> {
        self.emit(|store_name, tags| CacheFlushing { store_name, tags })
            .await?;
        let result = self.get_store().flush().await?;
        if result {
            self.emit(|store_name, tags| CacheFlushed { store_name, tags })
                .await?;
        } else {
            self.emit(|store_name, tags| CacheFlushFailed { store_name, tags })
                .await?;
        }
        Ok(result)
    }
}

impl Contract for Repository {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            self.emit(|store_name, tags| RetrievingKey {
                store_name,
                key: key.into(),
                tags,
            })
            .await?;
            let value = self.get_store().get(key).await?;
            self.retrieved(key, value.as_ref()).await?;
            Ok(value)
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: Option<Ttl>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        match ttl {
            None => self.forever(key, value),
            Some(ttl) => match seconds(ttl) {
                Some(seconds) => Box::pin(self.put_seconds(key, value, Some(seconds))),
                None => self.forget(key),
            },
        }
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: Option<Ttl>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        match ttl {
            None => Box::pin(async move { self.get_store().add(key, value, None).await }),
            Some(ttl) => match seconds(ttl) {
                Some(seconds) => {
                    Box::pin(async move { self.get_store().add(key, value, Some(seconds)).await })
                }
                None => Box::pin(async { Ok(false) }),
            },
        }
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.put_seconds(key, value, None))
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: Ttl) -> CacheFuture<'a, CacheResult<bool>> {
        match seconds(ttl) {
            Some(seconds) => Box::pin(async move { self.get_store().touch(key, seconds).await }),
            None => self.forget(key),
        }
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            self.emit(|store_name, tags| RetrievingKey {
                store_name,
                key: key.into(),
                tags,
            })
            .await?;
            self.emit(|store_name, tags| ForgettingKey {
                store_name,
                key: key.into(),
                tags,
            })
            .await?;
            let value = self.get_store().pull(key).await?;
            self.retrieved(key, value.as_ref()).await?;
            self.forgotten(key, value.is_some()).await?;
            Ok(value)
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.emit(|store_name, tags| ForgettingKey {
                store_name,
                key: key.into(),
                tags,
            })
            .await?;
            let result = self.get_store().forget(key).await?;
            self.forgotten(key, result).await?;
            Ok(result)
        })
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(async move { self.get_store().increment(key, amount).await })
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(async move { self.get_store().decrement(key, amount).await })
    }

    fn get_store(&self) -> Arc<dyn Store> {
        Repository::get_store(self)
    }
}

impl LockProvider for Repository {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        self.get_store()
            .lock_provider()
            .ok_or(CacheError::LocksUnsupported)?
            .lock(name, lease, owner)
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.get_store()
            .lock_provider()
            .ok_or(CacheError::LocksUnsupported)?
            .restore_lock(name, owner)
    }
}

pub(super) fn seconds(ttl: Ttl) -> Option<NonZeroU64> {
    match ttl {
        Ttl::Seconds(value) => NonZeroU64::new(value.try_into().ok()?),
        Ttl::Until(time) => {
            let duration = time.duration_since(SystemTime::now()).ok()?;
            NonZeroU64::new(
                duration
                    .as_secs()
                    .saturating_add(u64::from(duration.subsec_nanos() > 0)),
            )
        }
    }
}
