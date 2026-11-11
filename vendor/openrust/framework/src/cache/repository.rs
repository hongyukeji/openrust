use std::{
    future::Future,
    num::NonZeroU64,
    sync::Arc,
    time::{Duration, SystemTime},
};

use crate::contracts::cache::{Lock, LockProvider, Repository as Contract, Store};

use super::{CacheError, CacheFuture, CacheResult, Ttl, Value};

/// Applies application-level semantics to a cache store.
pub struct Repository(Arc<dyn Store>);

impl Repository {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self(store)
    }

    pub async fn has(&self, key: &str) -> CacheResult<bool> {
        Ok(self.get(key).await?.is_some_and(|value| !value.is_null()))
    }

    pub async fn missing(&self, key: &str) -> CacheResult<bool> {
        Ok(!self.has(key).await?)
    }

    pub async fn remember(
        &self,
        key: &str,
        ttl: Option<Ttl>,
        resolve: impl Future<Output = CacheResult<Value>>,
    ) -> CacheResult<Value> {
        if let Some(value) = self.get(key).await?.filter(|value| !value.is_null()) {
            return Ok(value);
        }
        let value = resolve.await?;
        self.put(key, &value, ttl).await?;
        Ok(value)
    }

    pub async fn flush(&self) -> CacheResult<bool> {
        self.0.flush().await
    }
}

impl Contract for Repository {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        self.0.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: Option<Ttl>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        match ttl {
            None => self.0.forever(key, value),
            Some(ttl) => match seconds(ttl) {
                Some(seconds) => self.0.put(key, value, seconds),
                None => self.0.forget(key),
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
            None => self.0.add(key, value, None),
            Some(ttl) => match seconds(ttl) {
                Some(seconds) => self.0.add(key, value, Some(seconds)),
                None => Box::pin(async { Ok(false) }),
            },
        }
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        self.0.forever(key, value)
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: Ttl) -> CacheFuture<'a, CacheResult<bool>> {
        match seconds(ttl) {
            Some(seconds) => self.0.touch(key, seconds),
            None => self.0.forget(key),
        }
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        self.0.pull(key)
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        self.0.forget(key)
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        self.0.increment(key, amount)
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        self.0.decrement(key, amount)
    }

    fn get_store(&self) -> &dyn Store {
        self.0.as_ref()
    }
}

impl LockProvider for Repository {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        self.0
            .lock_provider()
            .ok_or(CacheError::LocksUnsupported)?
            .lock(name, lease, owner)
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.0
            .lock_provider()
            .ok_or(CacheError::LocksUnsupported)?
            .restore_lock(name, owner)
    }
}

fn seconds(ttl: Ttl) -> Option<NonZeroU64> {
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
