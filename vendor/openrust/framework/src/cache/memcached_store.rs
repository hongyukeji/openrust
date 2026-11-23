use std::{num::NonZeroU64, sync::Arc, time::Duration};

use crate::contracts::cache::{Lock, LockProvider, Store};

use super::{
    CacheError, CacheFuture, CacheResult, MemcachedLock, TaggedCache, Value, backend,
    memcached_connector::{
        ADD, Client, DECREMENT, DELETE, GET, INCREMENT, SET, TOUCH, expiration, validate_key,
    },
};

/// Stores cache values and lock owners on Memcached.
pub struct MemcachedStore {
    client: Arc<Client>,
    prefix: String,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl MemcachedStore {
    pub fn new(client: Arc<Client>, prefix: String) -> Self {
        Self { client, prefix }
    }

    pub fn getMemcached(&self) -> &Client {
        &self.client
    }

    pub fn getPrefix(&self) -> &str {
        &self.prefix
    }

    pub fn setPrefix(&mut self, prefix: String) {
        self.prefix = prefix;
    }

    fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    async fn write(&self, opcode: u8, key: &str, value: &Value, seconds: u64) -> CacheResult<bool> {
        let mut extras = vec![0; 4];
        extras.extend_from_slice(&expiration(seconds)?.to_be_bytes());
        self.client
            .command(opcode, self.key(key), extras, encode(value)?, 0)
            .await?
            .mutation(if opcode == ADD { &[2, 5] } else { &[] })
    }

    async fn counter(&self, opcode: u8, key: &str, amount: i64) -> CacheResult<i64> {
        let amount = u64::try_from(amount).map_err(|_| CacheError::InvalidValue)?;
        let mut extras = Vec::with_capacity(20);
        extras.extend_from_slice(&amount.to_be_bytes());
        extras.extend_from_slice(&0_u64.to_be_bytes());
        extras.extend_from_slice(&u32::MAX.to_be_bytes());
        self.client
            .command(opcode, self.key(key), extras, Vec::new(), 0)
            .await?
            .counter()
    }
}

impl Store for MemcachedStore {
    fn tags(self: Arc<Self>, names: &[&str]) -> CacheResult<TaggedCache> {
        Ok(TaggedCache::new(self, names))
    }

    fn supports_tags(&self) -> bool {
        true
    }

    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            self.client
                .command(GET, self.key(key), Vec::new(), Vec::new(), 0)
                .await?
                .item()?
                .map(|item| decode(&item.value))
                .transpose()
        })
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move {
            self.client
                .many(keys.iter().map(|key| self.key(key)).collect())
                .await?
                .into_iter()
                .map(|value| value.as_deref().map(decode).transpose())
                .collect()
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(SET, key, value, ttl.get()))
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        ttl: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            for (key, _) in values {
                validate_key(&self.key(key))?;
            }
            for (key, value) in values {
                self.write(SET, key, value, ttl.get()).await?;
            }
            Ok(true)
        })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(ADD, key, value, ttl.map_or(0, NonZeroU64::get)))
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            let key = self.key(key);
            for _ in 0..16 {
                let Some(item) = self
                    .client
                    .command(GET, key.clone(), Vec::new(), Vec::new(), 0)
                    .await?
                    .item()?
                else {
                    return Ok(None);
                };
                let value = decode(&item.value)?;
                if self
                    .client
                    .command(DELETE, key.clone(), Vec::new(), Vec::new(), item.cas)
                    .await?
                    .mutation(&[1, 2])?
                {
                    return Ok(Some(value));
                }
            }
            Err(backend(std::io::Error::other(
                "Memcached item changed during pull",
            )))
        })
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(SET, key, value, 0))
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: NonZeroU64) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.client
                .command(
                    TOUCH,
                    self.key(key),
                    expiration(ttl.get())?.to_be_bytes().into(),
                    Vec::new(),
                    0,
                )
                .await?
                .mutation(&[1])
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.client
                .command(DELETE, self.key(key), Vec::new(), Vec::new(), 0)
                .await?
                .mutation(&[1])
        })
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(INCREMENT, key, amount))
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(DECREMENT, key, amount))
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move { self.client.flush().await.map(|()| true) })
    }

    fn get_prefix(&self) -> &str {
        &self.prefix
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        Some(self)
    }
}

impl LockProvider for MemcachedStore {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        Ok(Box::new(MemcachedLock::new(
            self.client.clone(),
            self.key(name),
            lease,
            owner,
        )?))
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.lock(name, None, Some(owner))
    }
}

fn encode(value: &Value) -> CacheResult<Vec<u8>> {
    match value {
        Value::Json(value) if value.is_u64() => Ok(value.to_string().into_bytes()),
        Value::Json(value) => {
            let mut bytes = vec![b'j'];
            serde_json::to_writer(&mut bytes, value).map_err(backend)?;
            Ok(bytes)
        }
        Value::Bytes(value) => {
            let mut bytes = Vec::with_capacity(value.len() + 1);
            bytes.push(b'b');
            bytes.extend_from_slice(value);
            Ok(bytes)
        }
    }
}

fn decode(bytes: &[u8]) -> CacheResult<Value> {
    match bytes.split_first() {
        Some((b'j', payload)) => serde_json::from_slice(payload)
            .map(Value::Json)
            .map_err(backend),
        Some((b'b', payload)) => Ok(Value::Bytes(payload.to_vec())),
        _ => {
            let value = std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
                .ok_or(CacheError::InvalidValue)?;
            Ok(Value::Json(value.into()))
        }
    }
}
