use std::time::Duration;

use crate::cache::{CacheError, CacheFuture, CacheResult};

/// An owner-aware atomic cache lock.
pub trait Lock: Send {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>>;
    fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>>;
    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>>;
    fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>>;
    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>>;
    fn owner(&self) -> &str;

    /// Reads the current owner without acquiring or renewing the lease.
    fn current_owner(&mut self) -> CacheFuture<'_, CacheResult<Option<String>>> {
        Box::pin(async { Err(CacheError::LockInspectionUnsupported) })
    }

    /// Returns whether the backend currently holds an active lock.
    #[allow(non_snake_case, reason = "Matches Laravel's lock API")]
    fn isLocked(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move { Ok(self.current_owner().await?.is_some()) })
    }

    /// Exposes the delay between attempts when this driver supports customization.
    fn retry_interval_mut(&mut self) -> Option<&mut Duration> {
        None
    }
}
