use std::{num::NonZeroU64, sync::Arc};

use ::redis::ToRedisArgs;

use crate::contracts::cache::{LockProvider, Store};

use super::{
    CacheError, CacheFuture, CacheResult, TagSet, Value,
    redis_store::{decode, encode},
    redis_tag_set::RedisTagSet,
};

pub(super) struct RedisTaggedCache(Arc<TagSet>);

impl RedisTaggedCache {
    pub(super) fn new(tags: Arc<TagSet>) -> Self {
        Self(tags)
    }

    fn tags(&self) -> &RedisTagSet {
        self.0.redis.as_ref().expect("Redis tag set")
    }

    async fn read(&self, operation: &str, key: &str) -> CacheResult<Option<Value>> {
        let bytes: Option<Vec<u8>> = self.tags().item(operation, key, "", "").await?;
        bytes.as_deref().map(decode).transpose()
    }

    async fn write(
        &self,
        operation: &str,
        key: &str,
        value: &Value,
        ttl: Option<NonZeroU64>,
    ) -> CacheResult<bool> {
        let seconds = ttl.map_or_else(String::new, |seconds| seconds.to_string());
        self.tags()
            .item(operation, key, encode(value)?, seconds)
            .await
    }

    async fn change(
        &self,
        operation: &str,
        key: &str,
        amount: impl ToRedisArgs + Send,
    ) -> CacheResult<i64> {
        let result: String = self.tags().item(operation, key, amount, "").await?;
        result.parse().map_err(|_| CacheError::InvalidValue)
    }
}

impl Store for RedisTaggedCache {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(self.read("get", key))
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move {
            let mut result = Vec::with_capacity(keys.len());
            for key in keys {
                result.push(self.read("get", key).await?);
            }
            Ok(result)
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write("put", key, value, Some(seconds)))
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            for (key, value) in values {
                self.write("put", key, value, Some(seconds)).await?;
            }
            Ok(true)
        })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write("add", key, value, seconds))
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(self.read("pull", key))
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write("put", key, value, None))
    }

    fn touch<'a>(
        &'a self,
        key: &'a str,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.tags().item("touch", key, seconds.get(), ""))
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.tags().item("forget", key, "", ""))
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.change("increment", key, amount))
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.change("decrement", key, amount))
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            self.0.reset().await?;
            Ok(true)
        })
    }

    fn get_prefix(&self) -> std::borrow::Cow<'_, str> {
        self.0.store.get_prefix()
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        self.0.store.lock_provider()
    }
}
