use std::{sync::Arc, time::Duration};

use crate::contracts::cache::Lock;

use super::{
    CacheError, CacheFuture, CacheResult,
    lock::{DEFAULT_RETRY_INTERVAL, block_lock, lock_owner},
    memcached_connector::{ADD, Client, DELETE, GET, Item, SET, validate_key},
};

/// An owner-checked atomic Memcached lock.
pub struct MemcachedLock {
    client: Arc<Client>,
    key: String,
    owner: String,
    lease: Option<Duration>,
    retry_interval: Duration,
}

impl MemcachedLock {
    pub(super) fn new(
        client: Arc<Client>,
        key: String,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Self> {
        validate_key(&key)?;
        Ok(Self {
            client,
            key,
            lease,
            owner: lock_owner(owner)?,
            retry_interval: DEFAULT_RETRY_INTERVAL,
        })
    }

    async fn read_item(&self) -> CacheResult<Option<Item>> {
        self.client
            .command(GET, self.key.clone(), Vec::new(), Vec::new(), 0)
            .await?
            .item()
    }

    async fn owned_version(&self) -> CacheResult<Option<u64>> {
        Ok(self
            .read_item()
            .await?
            .filter(|item| item.value == self.owner.as_bytes())
            .map(|item| item.cas))
    }

    async fn write(&self, opcode: u8, lease: Option<Duration>, cas: u64) -> CacheResult<bool> {
        let mut extras = vec![0; 4];
        extras.extend_from_slice(&lease_expiration(lease)?.to_be_bytes());
        self.client
            .command(
                opcode,
                self.key.clone(),
                extras,
                self.owner.as_bytes().to_vec(),
                cas,
            )
            .await?
            .mutation(&[1, 2, 5])
    }

    async fn delete(&self, cas: u64) -> CacheResult<bool> {
        self.client
            .command(DELETE, self.key.clone(), Vec::new(), Vec::new(), cas)
            .await?
            .mutation(&[1, 2])
    }
}

impl Lock for MemcachedLock {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(self.write(ADD, self.lease, 0))
    }

    fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>> {
        let interval = self.retry_interval;
        Box::pin(block_lock(self, wait, interval))
    }

    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            let Some(cas) = self.owned_version().await? else {
                return Ok(false);
            };
            self.delete(cas).await
        })
    }

    fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            let Some(cas) = self.owned_version().await? else {
                return Ok(false);
            };
            self.write(SET, lease.or(self.lease), cas).await
        })
    }

    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(async move { self.delete(0).await.map(|_| ()) })
    }

    fn current_owner(&mut self) -> CacheFuture<'_, CacheResult<Option<String>>> {
        Box::pin(async move {
            self.read_item()
                .await?
                .map(|item| String::from_utf8(item.value).map_err(|_| CacheError::InvalidOwner))
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

fn lease_expiration(lease: Option<Duration>) -> CacheResult<u32> {
    let lease = lease.unwrap_or_default();
    // The fixed the framework MemcachedLock converts leases beyond 10 days.
    let seconds = lease
        .as_secs()
        .checked_add(u64::from(lease.subsec_nanos() != 0))
        .ok_or(CacheError::InvalidValue)?;
    super::memcached_connector::lock_expiration(seconds)
}
