use std::{error::Error as _, num::NonZeroU64, sync::Arc, time::Duration};

use crate::{
    contracts::cache::{CanFlushLocks, Lock, LockProvider, Store},
    redis::connections::Connection,
};

use super::{
    CacheError, CacheFuture, CacheResult, RedisLock, TaggedCache, Value, backend,
    redis_tag_set::RedisTagSet,
};

const NUMERIC_COUNTER: &str = concat!(
    include_str!("scripts/redis_counter.lua"),
    "\nreturn integer_counter(KEYS[1], ARGV[1], ARGV[2])\n",
);

/// Cache storage for the Redis driver.
pub struct RedisStore {
    pub(super) data: Arc<Connection>,
    locks: Arc<Connection>,
    prefix: String,
}

impl RedisStore {
    pub fn new(data: Arc<Connection>, locks: Arc<Connection>, prefix: String) -> Self {
        Self {
            data,
            locks,
            prefix,
        }
    }

    pub(super) fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    async fn read(&self, key: &str, command: &str) -> CacheResult<Option<Value>> {
        let bytes: Option<Vec<u8>> = self
            .data
            .query(::redis::cmd(command).arg(self.data.key(&self.key(key))))
            .await
            .map_err(backend)?;
        bytes.as_deref().map(decode).transpose()
    }

    async fn write(
        &self,
        key: &str,
        value: &Value,
        ttl: Option<NonZeroU64>,
        add: bool,
    ) -> CacheResult<bool> {
        let mut command = ::redis::cmd("SET");
        command
            .arg(self.data.key(&self.key(key)))
            .arg(encode(value)?);
        if let Some(ttl) = ttl {
            command.arg("EX").arg(ttl.get());
        }
        if add {
            command.arg("NX");
        }
        let result: Option<String> = self.data.query(&command).await.map_err(backend)?;
        Ok(result.is_some())
    }

    async fn counter(&self, key: &str, command: &str, amount: i64) -> CacheResult<i64> {
        let key = self.data.key(&self.key(key));
        let result = self
            .data
            .query(::redis::cmd(command).arg(&key).arg(amount))
            .await;
        match result {
            Err(error)
                if error
                    .source()
                    .and_then(|cause| cause.downcast_ref::<::redis::RedisError>())
                    .and_then(::redis::RedisError::detail)
                    == Some("value is not an integer or out of range") =>
            {
                let value: String = self
                    .data
                    .query(
                        ::redis::cmd("EVAL")
                            .arg(NUMERIC_COUNTER)
                            .arg(1)
                            .arg(key)
                            .arg(command)
                            .arg(amount),
                    )
                    .await
                    .map_err(backend)?;
                value.parse().map_err(|_| CacheError::InvalidValue)
            }
            result => result.map_err(backend),
        }
    }
}

impl Store for RedisStore {
    fn tags(self: Arc<Self>, names: &[&str]) -> CacheResult<TaggedCache> {
        if self.data.is_cluster() {
            return Err(CacheError::TagsUnsupported);
        }
        Ok(TaggedCache::redis(self, names))
    }

    fn flush_stale_tags(&self) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(async move {
            if self.data.is_cluster() {
                return Err(CacheError::TagsUnsupported);
            }
            RedisTagSet::new(self, &[]).prune().await
        })
    }

    fn supports_tags(&self) -> bool {
        !self.data.is_cluster()
    }

    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(self.read(key, "GET"))
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move {
            if keys.is_empty() {
                return Ok(Vec::new());
            }
            let mut command = ::redis::cmd("MGET");
            for key in keys {
                command.arg(self.data.key(&self.key(key)));
            }
            let bytes: Vec<Option<Vec<u8>>> = self.data.query(&command).await.map_err(backend)?;
            bytes
                .iter()
                .map(|bytes| bytes.as_deref().map(decode).transpose())
                .collect()
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, value, Some(seconds), false))
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            if values.is_empty() {
                return Ok(true);
            }
            if self.data.is_cluster() {
                for (key, value) in values {
                    if !self.write(key, value, Some(seconds), false).await? {
                        return Ok(false);
                    }
                }
                return Ok(true);
            }
            let mut pipeline = ::redis::pipe();
            pipeline.atomic();
            for (key, value) in values {
                pipeline
                    .cmd("SET")
                    .arg(self.data.key(&self.key(key)))
                    .arg(encode(value)?)
                    .arg("EX")
                    .arg(seconds.get())
                    .ignore();
            }
            self.data.pipeline(&pipeline).await.map_err(backend)?;
            Ok(true)
        })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, value, seconds, true))
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(self.read(key, "GETDEL"))
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, value, None, false))
    }

    fn touch<'a>(
        &'a self,
        key: &'a str,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.data
                .query(
                    ::redis::cmd("EXPIRE")
                        .arg(self.data.key(&self.key(key)))
                        .arg(seconds.get()),
                )
                .await
                .map_err(backend)
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.data
                .query(::redis::cmd("DEL").arg(self.data.key(&self.key(key))))
                .await
                .map_err(backend)
        })
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(key, "INCRBY", amount))
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(key, "DECRBY", amount))
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            self.data
                .query::<()>(&::redis::cmd("FLUSHDB"))
                .await
                .map_err(backend)?;
            Ok(true)
        })
    }

    fn get_prefix(&self) -> &str {
        &self.prefix
    }

    fn lock_flusher(&self) -> Option<&dyn CanFlushLocks> {
        (!self.data.is_cluster() && !self.locks.is_cluster()).then_some(self)
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        Some(self)
    }
}

impl CanFlushLocks for RedisStore {
    fn hasSeparateLockStore(&self) -> bool {
        self.data
            .standalone_location()
            .zip(self.locks.standalone_location())
            .is_some_and(|(data, locks)| data != locks)
    }

    fn flushLocks(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            if !self.hasSeparateLockStore() {
                return Err(CacheError::LockStoreNotSeparate);
            }
            if self.data.standalone_location().map(|(index, _)| index)
                == self.locks.standalone_location().map(|(index, _)| index)
            {
                // Different connection addresses can lead to the same Redis database.
                let (data, locks) = tokio::try_join!(self.data.server_id(), self.locks.server_id())
                    .map_err(backend)?;
                if data == locks {
                    return Err(CacheError::LockStoreNotSeparate);
                }
            }
            self.locks
                .query::<()>(&::redis::cmd("FLUSHDB"))
                .await
                .map_err(backend)?;
            Ok(true)
        })
    }
}

impl LockProvider for RedisStore {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        Ok(Box::new(RedisLock::new(
            self.locks.clone(),
            self.locks.key(&self.key(name)),
            lease,
            owner,
        )?))
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.lock(name, None, Some(owner))
    }
}

pub(super) fn encode(value: &Value) -> CacheResult<Vec<u8>> {
    if let Some(integer) = value.as_i64() {
        return Ok(match value.as_str() {
            Some(text) => format!("n{integer}:{text}").into_bytes(),
            None => integer.to_string().into_bytes(),
        });
    }
    let mut bytes = vec![b'j'];
    serde_json::to_writer(&mut bytes, value).map_err(backend)?;
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8]) -> CacheResult<Value> {
    if let Some(payload) = bytes.strip_prefix(b"n") {
        let (integer, text) = std::str::from_utf8(payload)
            .ok()
            .and_then(|entry| entry.split_once(':'))
            .ok_or(CacheError::InvalidValue)?;
        let integer = integer
            .parse::<i64>()
            .map_err(|_| CacheError::InvalidValue)?;
        let result = Value::from(text);
        return (result.as_i64() == Some(integer))
            .then_some(result)
            .ok_or(CacheError::InvalidValue);
    }
    if let Some(payload) = bytes.strip_prefix(b"j") {
        return serde_json::from_slice(payload).map_err(backend);
    }
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .map(Value::from)
        .ok_or(CacheError::InvalidValue)
}
