use std::{sync::Arc, time::Duration};

use crate::{contracts::cache::Lock as LockContract, redis::connections::Connection};

use super::{
    CacheError, CacheFuture, CacheResult, backend,
    lock::{block_lock, lock_owner},
};

const RELEASE: &str = r#"
if redis.call('get', KEYS[1]) == ARGV[1] then
    return redis.call('del', KEYS[1])
end
return 0
"#;

const REFRESH: &str = r#"
if redis.call('get', KEYS[1]) ~= ARGV[1] then return 0 end
if ARGV[2] == '0' then
    redis.call('persist', KEYS[1])
else
    redis.call('pexpire', KEYS[1], ARGV[2])
end
return 1
"#;

/// An atomic lock for the Redis driver.
pub struct RedisLock {
    connection: Arc<Connection>,
    key: String,
    owner: String,
    lease: Option<Duration>,
}

impl RedisLock {
    pub(super) fn new(
        connection: Arc<Connection>,
        key: String,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Self> {
        Ok(Self {
            connection,
            key,
            lease,
            owner: lock_owner(owner)?,
        })
    }
}

impl LockContract for RedisLock {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            let mut command = ::redis::cmd("SET");
            command.arg(&self.key).arg(&self.owner).arg("NX");
            let lease = lease_millis(self.lease)?;
            if lease > 0 {
                command.arg("PX").arg(lease);
            }
            let result: Option<String> = self.connection.query(&command).await.map_err(backend)?;
            Ok(result.is_some())
        })
    }

    fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(block_lock(self, wait))
    }

    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            self.connection
                .query(
                    ::redis::cmd("EVAL")
                        .arg(RELEASE)
                        .arg(1)
                        .arg(&self.key)
                        .arg(&self.owner),
                )
                .await
                .map_err(backend)
        })
    }

    fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            let lease = lease_millis(lease.or(self.lease))?;
            self.connection
                .query(
                    ::redis::cmd("EVAL")
                        .arg(REFRESH)
                        .arg(1)
                        .arg(&self.key)
                        .arg(&self.owner)
                        .arg(lease),
                )
                .await
                .map_err(backend)
        })
    }

    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(async move {
            self.connection
                .query(::redis::cmd("DEL").arg(&self.key))
                .await
                .map_err(backend)
        })
    }

    fn owner(&self) -> &str {
        &self.owner
    }
}

fn lease_millis(lease: Option<Duration>) -> CacheResult<i64> {
    match lease {
        Some(lease) if !lease.is_zero() => {
            i64::try_from(lease.as_millis().max(1)).map_err(|_| CacheError::InvalidValue)
        }
        _ => Ok(0),
    }
}
