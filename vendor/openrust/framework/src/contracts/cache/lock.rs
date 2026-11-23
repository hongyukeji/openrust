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

    /// 按公开名称强制释放锁，不校验当前句柄的 owner。
    #[allow(non_snake_case)]
    fn forceRelease(&mut self) -> CacheFuture<'_, CacheResult<()>> {
        self.force_release()
    }

    /// 查询后端当前 owner，不获取或续租锁。
    #[allow(non_snake_case)]
    fn currentOwner(&mut self) -> CacheFuture<'_, CacheResult<Option<String>>> {
        self.current_owner()
    }

    /// Reads the current owner without acquiring or renewing the lease.
    fn current_owner(&mut self) -> CacheFuture<'_, CacheResult<Option<String>>> {
        Box::pin(async { Err(CacheError::LockInspectionUnsupported) })
    }

    /// Returns whether the backend currently holds an active lock.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn isLocked(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move { Ok(self.current_owner().await?.is_some()) })
    }

    /// Exposes the delay between attempts when this driver supports customization.
    fn retry_interval_mut(&mut self) -> Option<&mut Duration> {
        None
    }
}
