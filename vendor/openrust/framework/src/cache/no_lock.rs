use std::time::Duration;

use crate::contracts::cache::Lock;

use super::{
    CacheFuture, CacheResult,
    lock::{DEFAULT_RETRY_INTERVAL, lock_owner},
};

/// A lock handle for a cache that does not retain state.
pub struct NoLock {
    owner: String,
    retry_interval: Duration,
}

impl NoLock {
    pub fn new(owner: Option<&str>) -> CacheResult<Self> {
        Ok(Self {
            owner: lock_owner(owner)?,
            retry_interval: DEFAULT_RETRY_INTERVAL,
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's NoLock API")]
impl Lock for NoLock {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn block(&mut self, _: Duration) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(async { Ok(()) })
    }

    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn refresh(&mut self, _: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(async { Ok(()) })
    }

    fn current_owner(&mut self) -> CacheFuture<'_, CacheResult<Option<String>>> {
        Box::pin(async move { Ok(Some(self.owner.clone())) })
    }

    fn isLocked(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn retry_interval_mut(&mut self) -> Option<&mut Duration> {
        Some(&mut self.retry_interval)
    }

    fn owner(&self) -> &str {
        &self.owner
    }
}
