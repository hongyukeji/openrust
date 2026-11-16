use std::time::Duration;

use crate::contracts::cache::Lock;

use super::{CacheFuture, CacheResult, lock::lock_owner};

/// A lock handle for a cache that does not retain state.
pub struct NoLock {
    owner: String,
}

impl NoLock {
    pub fn new(owner: Option<&str>) -> CacheResult<Self> {
        Ok(Self {
            owner: lock_owner(owner)?,
        })
    }
}

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

    fn owner(&self) -> &str {
        &self.owner
    }
}
