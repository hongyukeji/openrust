use std::time::Duration;

use crate::{
    cache::{CacheError, CacheResult},
    contracts::cache::{Lock, LockProvider},
    http::HttpError,
    support::facades::Cache,
};

#[derive(Clone, Copy)]
pub(crate) struct RouteBlock {
    pub lock_seconds: u64,
    pub wait_seconds: u64,
}

/// Retains ownership through session persistence, including response errors.
pub(crate) struct SessionLock(Option<Box<dyn Lock>>);

impl SessionLock {
    pub async fn acquire(
        store: Option<&str>,
        id: &str,
        lease_seconds: u64,
        wait_seconds: u64,
    ) -> CacheResult<Self> {
        let lease = Duration::from_secs(lease_seconds);
        if lease.is_zero() {
            return Err(CacheError::InvalidValue);
        }
        let lock = Cache::store(store)?.lock(&format!("session:{id}"), Some(lease), None)?;
        // The guard is active even while the cache acquisition is in flight.
        let mut guard = Self(Some(lock));
        let lock = guard.0.as_mut().expect("session lock is present");
        let deadline = tokio::time::Instant::now()
            .checked_add(Duration::from_secs(wait_seconds))
            .ok_or(CacheError::InvalidValue)?;
        loop {
            if lock.get().await? {
                return Ok(guard);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(CacheError::LockTimeout);
            }
            tokio::time::sleep(remaining.min(Duration::from_millis(50))).await;
            if tokio::time::Instant::now() >= deadline {
                return Err(CacheError::LockTimeout);
            }
        }
    }

    pub async fn release(mut self) -> Result<(), HttpError> {
        if let Some(lock) = &mut self.0 {
            lock.release().await.map_err(http_error)?;
        }
        self.0 = None;
        Ok(())
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        if let Some(mut lock) = self.0.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                let _ = lock.release().await;
            });
        }
    }
}

pub(crate) fn http_error(source: CacheError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
