use crate::cache::{CacheFuture, CacheResult};

/// Removes locks from stores with a dedicated lock backend.
#[allow(non_snake_case, reason = "Matches Laravel's CanFlushLocks API")]
pub trait CanFlushLocks: Send + Sync {
    fn flushLocks(&self) -> CacheFuture<'_, CacheResult<bool>>;

    fn hasSeparateLockStore(&self) -> bool;
}
