use crate::cache::{CacheFuture, CacheResult};

/// Removes locks from stores with a dedicated lock backend.
pub trait CanFlushLocks: Send + Sync {
    fn flush_locks(&self) -> CacheFuture<'_, CacheResult<bool>>;

    fn has_separate_lock_store(&self) -> bool;
}
