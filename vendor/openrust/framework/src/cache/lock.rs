use std::{
    fmt::Write as _,
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};

use tokio::runtime::Handle;

use crate::contracts::cache::Lock as LockContract;

use super::{
    CacheError, CacheFuture, CacheResult, Value, backend,
    engine::{Change, Engine, Record, expires},
};

#[allow(
    non_snake_case,
    reason = "Matches Laravel's callback-based lock operations"
)]
impl dyn LockContract {
    /// Compares the backend owner with the given token. `None` tests for no owner.
    pub async fn isOwnedBy<'a>(&mut self, owner: impl Into<Option<&'a str>>) -> CacheResult<bool> {
        Ok(self.current_owner().await?.as_deref() == owner.into())
    }

    /// Compares the backend owner with this handle's token.
    pub async fn isOwnedByCurrentProcess(&mut self) -> CacheResult<bool> {
        Ok(self.current_owner().await?.as_deref() == Some(self.owner()))
    }

    /// Sets the interval used by `block` and `blockWith` on this handle.
    pub fn betweenBlockedAttemptsSleepFor(
        mut self: Box<Self>,
        milliseconds: u64,
    ) -> CacheResult<Box<Self>> {
        *self
            .retry_interval_mut()
            .ok_or(CacheError::LockRetryIntervalUnsupported)? = Duration::from_millis(milliseconds);
        Ok(self)
    }

    /// Attempts once, returning `None` if another owner is holding the lock.
    pub async fn getWith<T, F>(
        self: Box<Self>,
        callback: impl FnOnce() -> F,
    ) -> CacheResult<Option<T>>
    where
        F: Future<Output = CacheResult<T>>,
    {
        let Some(lock) = acquire_scoped(self, None).await? else {
            return Ok(None);
        };
        run_locked(lock, callback).await.map(Some)
    }

    /// Waits for a lock, then runs the callback and releases its ownership.
    pub async fn blockWith<T, F>(
        self: Box<Self>,
        wait: Duration,
        callback: impl FnOnce() -> F,
    ) -> CacheResult<T>
    where
        F: Future<Output = CacheResult<T>>,
    {
        let lock = acquire_scoped(self, Some(wait))
            .await?
            .expect("a blocking acquisition cannot return a miss");
        run_locked(lock, callback).await
    }
}

pub(super) async fn run_locked<T, F>(
    lock: ScopedLock,
    callback: impl FnOnce() -> F,
) -> CacheResult<T>
where
    F: Future<Output = CacheResult<T>>,
{
    let result = callback().await;
    lock.release().await?;
    result
}

pub(super) async fn acquire_scoped(
    mut lock: Box<dyn LockContract>,
    wait: Option<Duration>,
) -> CacheResult<Option<ScopedLock>> {
    let runtime = Handle::try_current().ok();
    let release_runtime = runtime.clone();
    let acquire = async move {
        let acquired = match wait {
            Some(wait) => {
                lock.block(wait).await?;
                true
            }
            None => lock.get().await?,
        };
        if acquired {
            Ok(Some(ScopedLock {
                lock: Some(lock),
                runtime: release_runtime,
            }))
        } else {
            Ok(None)
        }
    };
    let mut pending = PendingAcquisition {
        future: Some(Box::pin(acquire)),
        runtime,
    };
    poll_fn(|context| {
        let mut future = pending.future.take().expect("pending acquisition");
        match future.as_mut().poll(context) {
            Poll::Ready(result) => Poll::Ready(result),
            Poll::Pending => {
                pending.future = Some(future);
                Poll::Pending
            }
        }
    })
    .await
}

struct PendingAcquisition {
    future: Option<CacheFuture<'static, CacheResult<Option<ScopedLock>>>>,
    runtime: Option<Handle>,
}

impl Drop for PendingAcquisition {
    fn drop(&mut self) {
        if let Some(future) = self.future.take()
            && let Some(runtime) = &self.runtime
        {
            // The backend may already have granted the lock. Complete the acquisition
            // before releasing, so a contended attempt cannot release unowned state.
            runtime.spawn(async move {
                if let Ok(Some(lock)) = future.await {
                    let _ = lock.release().await;
                }
            });
        }
    }
}

pub(super) struct ScopedLock {
    lock: Option<Box<dyn LockContract>>,
    runtime: Option<Handle>,
}

impl ScopedLock {
    async fn release(mut self) -> CacheResult<()> {
        let result = self.lock.as_mut().expect("acquired lock").release().await;
        self.lock = None;
        result.map(|_| ())
    }
}

impl Drop for ScopedLock {
    fn drop(&mut self) {
        if let Some(mut lock) = self.lock.take()
            && let Some(runtime) = &self.runtime
        {
            runtime.spawn(async move {
                let _ = lock.release().await;
            });
        }
    }
}

pub(super) const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_millis(250);

/// Coordinates acquisition and ownership for an atomic cache lock.
pub struct Lock {
    engine: Engine,
    key: String,
    owner: String,
    lease: Option<Duration>,
    database: bool,
    retry_interval: Duration,
}

impl Lock {
    pub(super) fn new(
        engine: Engine,
        key: String,
        lease: Option<Duration>,
        owner: Option<&str>,
        database: bool,
    ) -> CacheResult<Self> {
        let owner = lock_owner(owner)?;
        Ok(Self {
            engine,
            key,
            owner,
            lease,
            database,
            retry_interval: DEFAULT_RETRY_INTERVAL,
        })
    }

    fn effective_lease(&self, lease: Option<Duration>) -> Option<Duration> {
        let lease = lease.or(self.lease).filter(|lease| !lease.is_zero());
        lease.or_else(|| self.database.then_some(Duration::from_secs(86_400)))
    }
}

impl LockContract for Lock {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        let lease = self.effective_lease(None);
        let owner = self.owner.clone();
        let database = self.database;
        Box::pin(self.engine.change(self.key.clone(), move |entry| {
            if entry.is_some_and(|entry| !database || entry.value.as_str() != Some(owner.as_str()))
            {
                return Ok((Change::Keep, false));
            }
            Ok((
                Change::Write(Record {
                    value: Value::from(owner.as_str()),
                    expires: expires(lease)?,
                }),
                true,
            ))
        }))
    }

    fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>> {
        let interval = self.retry_interval;
        Box::pin(block_lock(self, wait, interval))
    }

    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        let owner = self.owner.clone();
        Box::pin(self.engine.change(self.key.clone(), move |entry| {
            if entry.is_some_and(|entry| entry.value.as_str() == Some(owner.as_str())) {
                Ok((Change::Remove, true))
            } else {
                Ok((Change::Keep, false))
            }
        }))
    }

    fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>> {
        let owner = self.owner.clone();
        let lease = self.effective_lease(lease);
        Box::pin(self.engine.change(self.key.clone(), move |entry| {
            if let Some(mut entry) = entry
                && entry.value.as_str() == Some(owner.as_str())
            {
                entry.expires = expires(lease)?;
                return Ok((Change::Write(entry), true));
            }
            Ok((Change::Keep, false))
        }))
    }

    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(
            self.engine
                .change(self.key.clone(), |_| Ok((Change::Remove, ()))),
        )
    }

    fn current_owner(&mut self) -> CacheFuture<'_, CacheResult<Option<String>>> {
        Box::pin(async move {
            self.engine
                .read(self.key.clone())
                .await?
                .map(|record| {
                    record
                        .value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or(CacheError::InvalidOwner)
                })
                .transpose()
        })
    }

    fn retry_interval_mut(&mut self) -> Option<&mut Duration> {
        Some(&mut self.retry_interval)
    }

    fn owner(&self) -> &str {
        &self.owner
    }
}

pub(super) fn lock_owner(owner: Option<&str>) -> CacheResult<String> {
    Ok(match owner {
        Some(owner) if !owner.is_empty() && owner.len() <= 255 => owner.to_owned(),
        Some(_) => return Err(CacheError::InvalidOwner),
        None => {
            let mut bytes = [0; 20];
            aws_lc_rs::rand::fill(&mut bytes)
                .map_err(|_| backend(std::io::Error::other("lock owner generation failed")))?;
            let mut owner = String::with_capacity(40);
            for byte in bytes {
                let _ = write!(owner, "{byte:02x}");
            }
            owner
        }
    })
}

pub(super) async fn block_lock(
    lock: &mut dyn LockContract,
    wait: Duration,
    interval: Duration,
) -> CacheResult<()> {
    let deadline = tokio::time::Instant::now()
        .checked_add(wait)
        .ok_or(CacheError::InvalidValue)?;
    loop {
        if lock.get().await? {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(CacheError::LockTimeout);
        }
        if interval.is_zero() {
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(remaining.min(interval)).await;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CacheError::LockTimeout);
        }
    }
}
