use std::{num::NonZeroU64, time::Duration};

use crate::contracts::cache::{Lock, Store};

use super::{
    CacheError, CacheFuture, CacheResult, DynamoDbStore, Value,
    lock::{DEFAULT_RETRY_INTERVAL, block_lock, lock_owner},
};

/// An atomic lock leased through a DynamoDB cache item.
pub struct DynamoDbLock {
    store: DynamoDbStore,
    key: String,
    owner: String,
    lease: NonZeroU64,
    retry_interval: Duration,
}

impl DynamoDbLock {
    pub(super) fn new(
        store: DynamoDbStore,
        key: String,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Self> {
        Ok(Self {
            store,
            key,
            owner: lock_owner(owner)?,
            lease: lease_seconds(lease)?,
            retry_interval: DEFAULT_RETRY_INTERVAL,
        })
    }
}

impl Lock for DynamoDbLock {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            self.store
                .add(
                    &self.key,
                    &Value::from(self.owner.as_str()),
                    Some(self.lease),
                )
                .await
        })
    }

    fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>> {
        let interval = self.retry_interval;
        Box::pin(block_lock(self, wait, interval))
    }

    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(self.store.delete_if_owned(&self.key, &self.owner))
    }

    fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            let lease = match lease {
                Some(lease) => lease_seconds(Some(lease))?,
                None => self.lease,
            };
            self.store
                .refresh_if_owned(&self.key, &self.owner, lease)
                .await
        })
    }

    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(async move { self.store.forget(&self.key).await.map(|_| ()) })
    }

    fn current_owner(&mut self) -> CacheFuture<'_, CacheResult<Option<String>>> {
        Box::pin(self.store.read_lock_owner(&self.key))
    }

    fn retry_interval_mut(&mut self) -> Option<&mut Duration> {
        Some(&mut self.retry_interval)
    }

    fn owner(&self) -> &str {
        &self.owner
    }
}

fn lease_seconds(lease: Option<Duration>) -> CacheResult<NonZeroU64> {
    let lease = lease.unwrap_or_default();
    let seconds = lease
        .as_secs()
        .checked_add(u64::from(lease.subsec_nanos() != 0))
        .ok_or(CacheError::InvalidValue)?;
    Ok(NonZeroU64::new(seconds).unwrap_or(NonZeroU64::new(86_400).expect("24 hours")))
}
