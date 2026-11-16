use std::{num::NonZeroU64, sync::Arc, time::Duration};

use crate::contracts::cache::{CanFlushLocks, Lock, LockProvider, Store};

use super::{CacheError, CacheFuture, CacheResult, NoLock, TaggedCache, Value};

/// Discards cache writes and returns a miss for every key.
#[derive(Default)]
pub struct NullStore;

impl Store for NullStore {
    fn tags(self: Arc<Self>, names: &[&str]) -> CacheResult<TaggedCache> {
        Ok(TaggedCache::new(self, names))
    }

    fn supports_tags(&self) -> bool {
        true
    }

    fn get<'a>(&'a self, _: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async { Ok(None) })
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move { Ok(vec![None; keys.len()]) })
    }

    fn put<'a>(
        &'a self,
        _: &'a str,
        _: &'a Value,
        _: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn put_many<'a>(
        &'a self,
        _: &'a [(String, Value)],
        _: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn add<'a>(
        &'a self,
        _: &'a str,
        _: &'a Value,
        _: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn pull<'a>(&'a self, _: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async { Ok(None) })
    }

    fn forever<'a>(&'a self, _: &'a str, _: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn touch<'a>(&'a self, _: &'a str, _: NonZeroU64) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn forget<'a>(&'a self, _: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn increment<'a>(&'a self, _: &'a str, _: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(async { Err(CacheError::MissingCounter) })
    }

    fn decrement<'a>(&'a self, _: &'a str, _: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(async { Err(CacheError::MissingCounter) })
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn get_prefix(&self) -> &str {
        ""
    }

    fn lock_flusher(&self) -> Option<&dyn CanFlushLocks> {
        Some(self)
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        Some(self)
    }
}

impl CanFlushLocks for NullStore {
    fn flushLocks(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn hasSeparateLockStore(&self) -> bool {
        false
    }
}

impl LockProvider for NullStore {
    fn lock(
        &self,
        _: &str,
        _: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        Ok(Box::new(NoLock::new(owner)?))
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.lock(name, None, Some(owner))
    }
}
