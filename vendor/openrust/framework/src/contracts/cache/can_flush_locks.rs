use crate::cache::{CacheFuture, CacheResult};

/// Removes locks from stores with a dedicated lock backend.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub trait CanFlushLocks: Send + Sync {
    fn flushLocks(&self) -> CacheFuture<'_, CacheResult<bool>>;

    fn hasSeparateLockStore(&self) -> bool;
}
