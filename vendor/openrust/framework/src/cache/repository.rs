use std::{
    future::Future,
    num::NonZeroU64,
    sync::{Arc, Weak},
    time::{Duration, SystemTime},
};

use crate::{
    container::Container,
    contracts::{
        cache::{Lock, LockProvider, Repository as Contract, Store},
        container::Container as _,
        events::{Dispatcher as EventDispatcher, Event},
    },
    events::{Dispatcher, EventContext},
};

use super::{
    CacheError, CacheFuture, CacheResult, TaggedCache, Ttl, Value, backend, events::*,
    limiters::ConcurrencyLimiterBuilder,
};

/// Applies application-level semantics to a cache store.
#[derive(Clone)]
pub struct Repository {
    store: Arc<dyn Store>,
    name: Option<String>,
    default_cache_time: Option<i64>,
    dispatcher: Option<RepositoryDispatcher>,
    tags: Vec<String>,
}

#[derive(Clone)]
enum RepositoryDispatcher {
    Application(Weak<Container>),
    Explicit(Arc<dyn EventDispatcher>),
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Repository {
    /// Builds a concurrency limiter on this repository's store.
    pub fn funnel(&self, name: impl Into<String>) -> CacheResult<ConcurrencyLimiterBuilder> {
        self.store
            .lock_provider()
            .ok_or(CacheError::LocksUnsupported)?;
        Ok(ConcurrencyLimiterBuilder::new(
            Arc::clone(&self.store),
            name.into(),
        ))
    }

    /// Runs a single callback under an exclusive lock, waiting up to ten seconds.
    pub async fn withoutOverlapping<T, F>(
        &self,
        key: &str,
        callback: impl FnOnce() -> F,
    ) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        self.withoutOverlappingWith(key, callback, 0, 10, None)
            .await
    }

    /// Uses explicit lease and wait durations in seconds, with an optional owner token.
    pub async fn withoutOverlappingWith<T, F>(
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
            .blockWith(Duration::from_secs(wait_for), callback)
            .await
    }

    pub fn tags(&self, names: &[&str]) -> CacheResult<TaggedCache> {
        let mut tagged = Arc::clone(&self.store).tags(names)?;
        tagged.repository.name.clone_from(&self.name);
        tagged.repository.default_cache_time = self.default_cache_time;
        if self.dispatcher.is_some() {
            tagged.repository.dispatcher.clone_from(&self.dispatcher);
        }
        tagged.repository.tags = names.iter().map(|name| (*name).to_owned()).collect();
        Ok(tagged)
    }

    pub fn supportsTags(&self) -> bool {
        self.store.supports_tags()
    }

    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            name: None,
            default_cache_time: Some(3600),
            dispatcher: None,
            tags: Vec::new(),
        }
    }

    pub(super) fn memoized_view(&self, store: Arc<dyn Store>) -> Self {
        Self {
            store,
            dispatcher: None,
            ..self.clone()
        }
    }

    pub(super) async fn clear_store(&self, store: Arc<dyn Store>) -> CacheResult<bool> {
        let mut repository = self.clone();
        repository.store = store;
        repository.tags.clear();
        repository.flush().await
    }

    pub(super) fn with_name(mut self, name: Option<&str>) -> Self {
        self.name = name.map(str::to_owned);
        self
    }

    pub(super) fn with_events(mut self, owner: Weak<Container>) -> Self {
        self.dispatcher = Some(RepositoryDispatcher::Application(owner));
        self
    }

    /// Returns the name assigned when the repository was constructed.
    pub fn getName(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns a handle to this repository's storage backend.
    pub fn getStore(&self) -> Arc<dyn Store> {
        Arc::clone(&self.store)
    }

    /// Replaces the storage backend on a uniquely borrowed repository.
    pub fn setStore(&mut self, store: Arc<dyn Store>) -> &mut Self {
        self.store = store;
        self
    }

    /// Returns the current event dispatcher, if event delivery is configured.
    pub fn getEventDispatcher(&self) -> Option<Arc<dyn EventDispatcher>> {
        match self.dispatcher.as_ref()? {
            RepositoryDispatcher::Application(owner) => owner
                .upgrade()?
                .make::<Dispatcher>()
                .ok()
                .map(|dispatcher| dispatcher as Arc<dyn EventDispatcher>),
            RepositoryDispatcher::Explicit(dispatcher) => Some(Arc::clone(dispatcher)),
        }
    }

    /// Overrides event delivery for this repository.
    pub fn setEventDispatcher(&mut self, dispatcher: Arc<dyn EventDispatcher>) {
        self.dispatcher = Some(RepositoryDispatcher::Explicit(dispatcher));
    }

    /// Returns the lifetime used by `putWithDefault`, in seconds.
    pub fn getDefaultCacheTime(&self) -> Option<i64> {
        self.default_cache_time
    }

    /// Sets the default lifetime. `None` writes values indefinitely.
    pub fn setDefaultCacheTime(&mut self, seconds: Option<i64>) -> &mut Self {
        self.default_cache_time = seconds;
        self
    }

    /// Writes a value with this repository's default lifetime.
    pub async fn putWithDefault(&self, key: &str, value: &Value) -> CacheResult<bool> {
        self.put(key, value, self.default_cache_time.map(Ttl::Seconds))
            .await
    }

    async fn emit<E: Event>(
        &self,
        event: impl FnOnce(Option<String>, Vec<String>) -> E,
    ) -> CacheResult<()> {
        if let Some(dispatcher) = self.getEventDispatcher() {
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
                storeName: store_name,
                key: key.into(),
                tags,
                value: value.clone(),
            })
            .await
        } else {
            self.emit(|store_name, tags| CacheMissed {
                storeName: store_name,
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
                storeName: store_name,
                key: key.into(),
                tags,
                value: value.clone(),
                seconds,
            })
            .await
        } else {
            self.emit(|store_name, tags| KeyWriteFailed {
                storeName: store_name,
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
                storeName: store_name,
                key: key.into(),
                tags,
            })
            .await
        } else {
            self.emit(|store_name, tags| KeyForgetFailed {
                storeName: store_name,
                key: key.into(),
                tags,
            })
            .await
        }
    }

    pub async fn many(&self, keys: &[&str]) -> CacheResult<Vec<Option<Value>>> {
        self.emit(|store_name, tags| RetrievingManyKeys {
            storeName: store_name,
            tags,
            key: keys.first().copied().unwrap_or_default().into(),
            keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        })
        .await?;
        let values = self.store.many(keys).await?;
        for (key, value) in keys.iter().zip(&values) {
            self.retrieved(key, value.as_ref()).await?;
        }
        Ok(values)
    }

    pub async fn putMany(&self, values: &[(String, Value)], ttl: Option<Ttl>) -> CacheResult<bool> {
        match ttl {
            Some(ttl) => match seconds(ttl) {
                Some(seconds) => self.put_many_seconds(values, seconds).await,
                None => {
                    let keys: Vec<_> = values.iter().map(|(key, _)| key.as_str()).collect();
                    self.deleteMultiple(&keys).await
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
            storeName: store_name,
            key: key.into(),
            tags,
            value: value.clone(),
            seconds: seconds.map(NonZeroU64::get),
        })
        .await?;
        let result = match seconds {
            Some(seconds) => self.store.put(key, value, seconds).await?,
            None => self.store.forever(key, value).await?,
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
            storeName: store_name,
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
        let result = self.store.put_many(entries, seconds).await?;
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
        self.getOrElse(key, || async { Ok(default) }).await
    }

    /// Resolves the default only when the key is missing or contains null.
    pub async fn getOrElse<F>(&self, key: &str, default: impl FnOnce() -> F) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        match self.get(key).await?.filter(|value| !value.is_null()) {
            Some(value) => Ok(value),
            None => default().await,
        }
    }

    /// Atomically removes the entry, returning a default if it was missing or null.
    pub async fn pullOr(&self, key: &str, default: Value) -> CacheResult<Value> {
        self.pullOrElse(key, || async { Ok(default) }).await
    }

    /// Atomically removes the entry before resolving the default on a miss.
    pub async fn pullOrElse<F>(&self, key: &str, default: impl FnOnce() -> F) -> CacheResult<Value>
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
        self.rememberWithWarmth(key, ttl, resolve)
            .await
            .map(|(value, _)| value)
    }

    /// Returns the cached value or resolves it, with a flag indicating a non-null hit.
    pub async fn rememberWithWarmth(
        &self,
        key: &str,
        ttl: Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<(Value, bool)> {
        self.rememberWithWarmthFor(key, |_| ttl, resolve).await
    }

    /// Computes the expiry from the resolved value on a miss.
    pub async fn rememberWithWarmthFor(
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
    pub async fn rememberForever(
        &self,
        key: &str,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        self.remember(key, None, resolve).await
    }

    /// Alias for `rememberForever`.
    pub async fn sear(
        &self,
        key: &str,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        self.rememberForever(key, resolve).await
    }

    pub fn supportsFlushingLocks(&self) -> bool {
        self.store.lock_flusher().is_some()
    }

    pub async fn flushLocks(&self) -> CacheResult<bool> {
        let store = self
            .store
            .lock_flusher()
            .ok_or(CacheError::LockFlushUnsupported)?;
        self.emit(|store_name, _| CacheLocksFlushing {
            storeName: store_name,
        })
        .await?;
        let result = store.flushLocks().await?;
        if result {
            self.emit(|store_name, _| CacheLocksFlushed {
                storeName: store_name,
            })
            .await?;
        } else {
            self.emit(|store_name, _| CacheLocksFlushFailed {
                storeName: store_name,
            })
            .await?;
        }
        Ok(result)
    }

    pub async fn flush(&self) -> CacheResult<bool> {
        self.emit(|store_name, tags| CacheFlushing {
            storeName: store_name,
            tags,
        })
        .await?;
        let result = self.store.flush().await?;
        if result {
            self.emit(|store_name, tags| CacheFlushed {
                storeName: store_name,
                tags,
            })
            .await?;
        } else {
            self.emit(|store_name, tags| CacheFlushFailed {
                storeName: store_name,
                tags,
            })
            .await?;
        }
        Ok(result)
    }
}

impl Contract for Repository {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            self.emit(|store_name, tags| RetrievingKey {
                storeName: store_name,
                key: key.into(),
                tags,
            })
            .await?;
            let value = self.store.get(key).await?;
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
            None => self.store.add(key, value, None),
            Some(ttl) => match seconds(ttl) {
                Some(seconds) => self.store.add(key, value, Some(seconds)),
                None => Box::pin(async { Ok(false) }),
            },
        }
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.put_seconds(key, value, None))
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: Ttl) -> CacheFuture<'a, CacheResult<bool>> {
        match seconds(ttl) {
            Some(seconds) => self.store.touch(key, seconds),
            None => self.forget(key),
        }
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            self.emit(|store_name, tags| RetrievingKey {
                storeName: store_name,
                key: key.into(),
                tags,
            })
            .await?;
            self.emit(|store_name, tags| ForgettingKey {
                storeName: store_name,
                key: key.into(),
                tags,
            })
            .await?;
            let value = self.store.pull(key).await?;
            self.retrieved(key, value.as_ref()).await?;
            self.forgotten(key, value.is_some()).await?;
            Ok(value)
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.emit(|store_name, tags| ForgettingKey {
                storeName: store_name,
                key: key.into(),
                tags,
            })
            .await?;
            let result = self.store.forget(key).await?;
            self.forgotten(key, result).await?;
            Ok(result)
        })
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        self.store.increment(key, amount)
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        self.store.decrement(key, amount)
    }

    fn get_store(&self) -> &dyn Store {
        self.store.as_ref()
    }
}

impl LockProvider for Repository {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        self.store
            .lock_provider()
            .ok_or(CacheError::LocksUnsupported)?
            .lock(name, lease, owner)
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.store
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
