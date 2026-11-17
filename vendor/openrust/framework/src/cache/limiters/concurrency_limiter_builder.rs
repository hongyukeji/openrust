use std::{future::Future, sync::Arc, time::Duration};

use tokio::time::Instant;

use crate::contracts::cache::Store;

use crate::cache::{
    CacheError, CacheResult,
    lock::{DEFAULT_RETRY_INTERVAL, ScopedLock, acquire_scoped, lock_owner, run_locked},
};

/// Bounds concurrent callbacks across callers using the same lock store and name.
#[must_use]
pub struct ConcurrencyLimiterBuilder {
    store: Arc<dyn Store>,
    name: String,
    max_locks: usize,
    lease: Duration,
    wait: Duration,
    interval: Duration,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's cache concurrency limiter API"
)]
impl ConcurrencyLimiterBuilder {
    pub(in crate::cache) fn new(store: Arc<dyn Store>, name: String) -> Self {
        Self {
            store,
            name,
            max_locks: 0,
            lease: Duration::from_secs(60),
            wait: Duration::from_secs(3),
            interval: DEFAULT_RETRY_INTERVAL,
        }
    }

    /// Sets the number of available slots.
    pub fn limit(mut self, max_locks: usize) -> Self {
        self.max_locks = max_locks;
        self
    }

    /// Sets the slot lease in seconds.
    pub fn releaseAfter(mut self, seconds: u64) -> Self {
        self.lease = Duration::from_secs(seconds);
        self
    }

    /// Sets how long to wait for an available slot, in seconds.
    pub fn block(mut self, seconds: u64) -> Self {
        self.wait = Duration::from_secs(seconds);
        self
    }

    /// Sets the retry interval in milliseconds.
    pub fn sleep(mut self, milliseconds: u64) -> Self {
        self.interval = Duration::from_millis(milliseconds);
        self
    }

    /// Runs the callback under a slot lease, or returns `CacheError::LimiterTimeout`.
    pub async fn then<T, F>(self, callback: impl FnOnce() -> F) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        let lock = self.acquire().await?.ok_or(CacheError::LimiterTimeout)?;
        run_locked(lock, callback).await
    }

    /// Runs the fallback when the wait for a slot expires.
    pub async fn thenWith<T, F, G>(
        self,
        callback: impl FnOnce() -> F,
        fallback: impl FnOnce(CacheError) -> G,
    ) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
        G: Future<Output = CacheResult<T>>,
    {
        match self.then(callback).await {
            Err(error @ CacheError::LimiterTimeout) => fallback(error).await,
            result => result,
        }
    }

    async fn acquire(&self) -> CacheResult<Option<ScopedLock>> {
        let provider = self
            .store
            .lock_provider()
            .ok_or(CacheError::LocksUnsupported)?;
        let deadline = Instant::now()
            .checked_add(self.wait)
            .ok_or(CacheError::InvalidValue)?;
        let owner = lock_owner(None)?;
        loop {
            for slot in 1..=self.max_locks {
                let key = format!("{}{slot}", self.name);
                let lock = provider.lock(&key, Some(self.lease), Some(&owner))?;
                if let Some(lock) = acquire_scoped(lock, None).await? {
                    return Ok(Some(lock));
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            if self.interval.is_zero() {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(remaining.min(self.interval)).await;
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }
}
