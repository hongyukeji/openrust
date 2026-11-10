use std::time::Duration;

use crate::cache::{CacheFuture, CacheResult};

/// An owner-aware atomic cache lock.
pub trait Lock: Send {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>>;
    fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>>;
    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>>;
    fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>>;
    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>>;
    fn owner(&self) -> &str;
}
