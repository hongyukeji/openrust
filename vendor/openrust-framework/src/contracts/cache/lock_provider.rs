use std::time::Duration;

use crate::cache::CacheResult;

use super::Lock;

/// Creates locks and restores handles to a known owner.
pub trait LockProvider: Send + Sync {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>>;
    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>>;
}
