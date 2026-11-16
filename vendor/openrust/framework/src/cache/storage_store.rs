use std::{fmt::Write as _, num::NonZeroU64, sync::Arc, time::Duration};

use aws_lc_rs::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};

use crate::{
    contracts::cache::{LockProvider, Store},
    filesystem::{FilesystemAdapter, FilesystemError},
};

use super::{
    CacheError, CacheFuture, CacheResult, Value, backend,
    engine::{FOREVER, Record, expires, now},
};

/// Stores cache records on a configured filesystem disk.
pub struct StorageStore {
    disk: Arc<FilesystemAdapter>,
    directory: String,
    prefix: String,
}

#[allow(non_snake_case, reason = "Matches Laravel's storage cache API")]
impl StorageStore {
    pub fn new(disk: Arc<FilesystemAdapter>, directory: &str, prefix: String) -> CacheResult<Self> {
        let directory = directory.trim_matches('/').to_owned();
        if !directory.is_empty() {
            disk.path(&directory).map_err(backend)?;
        }
        Ok(Self {
            disk,
            directory,
            prefix,
        })
    }

    pub fn getDisk(&self) -> &FilesystemAdapter {
        &self.disk
    }

    pub fn getDirectory(&self) -> &str {
        &self.directory
    }

    pub fn getPrefix(&self) -> &str {
        &self.prefix
    }

    pub fn setPrefix(&mut self, prefix: String) {
        self.prefix = prefix;
    }

    pub fn path(&self, key: &str) -> String {
        let key = format!("{}{key}", self.prefix);
        let mut hash = String::with_capacity(40);
        for byte in digest(&SHA1_FOR_LEGACY_USE_ONLY, key.as_bytes()).as_ref() {
            let _ = write!(hash, "{byte:02x}");
        }
        let shard = format!("{}/{}/{hash}", &hash[..2], &hash[2..4]);
        if self.directory.is_empty() {
            shard
        } else {
            format!("{}/{shard}", self.directory)
        }
    }

    async fn read(&self, key: &str) -> CacheResult<Option<Record>> {
        let Some(bytes) = self.disk.get(&self.path(key)).await.ok().flatten() else {
            return Ok(None);
        };
        let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
            return Ok(None);
        };
        // A disk read does not own the key. A concurrent write may have replaced the old record.
        Ok((record.expires > now()?).then_some(record))
    }

    async fn write(&self, key: &str, value: &Value, expiration: i64) -> CacheResult<bool> {
        #[derive(serde::Serialize)]
        struct Payload<'a> {
            value: &'a Value,
            expires: i64,
        }
        let bytes = serde_json::to_vec(&Payload {
            value,
            expires: expiration,
        })
        .map_err(backend)?;
        self.disk.put(&self.path(key), bytes).await.map_err(backend)
    }

    async fn counter(
        &self,
        key: &str,
        update: impl FnOnce(i64) -> Option<i64>,
    ) -> CacheResult<i64> {
        let record = self.read(key).await?;
        let current = match record
            .as_ref()
            .map(|record| &record.value)
            .filter(|value| !value.is_null())
        {
            Some(value) => value.as_i64().ok_or(CacheError::InvalidValue)?,
            None => 0,
        };
        let result = update(current).ok_or(CacheError::InvalidValue)?;
        let expiration = record.map_or(FOREVER, |record| record.expires);
        self.write(key, &Value::from(result), expiration).await?;
        Ok(result)
    }
}

impl Store for StorageStore {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move { Ok(self.read(key).await?.map(|record| record.value)) })
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move {
            let mut values = Vec::with_capacity(keys.len());
            for key in keys {
                values.push(self.get(key).await?);
            }
            Ok(values)
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.write(
                key,
                value,
                expires(Some(Duration::from_secs(seconds.get())))?,
            )
            .await
        })
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let expiration = expires(Some(Duration::from_secs(seconds.get())))?;
            let mut written = !values.is_empty();
            for (key, value) in values {
                written &= self.write(key, value, expiration).await?;
            }
            Ok(written)
        })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            if self.get(key).await?.is_some_and(|value| !value.is_null()) {
                return Ok(false);
            }
            let lease = seconds.map(|seconds| Duration::from_secs(seconds.get()));
            self.write(key, value, expires(lease)?).await
        })
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            let value = self.get(key).await?;
            self.forget(key).await?;
            Ok(value)
        })
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, value, FOREVER))
    }

    fn touch<'a>(
        &'a self,
        key: &'a str,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            match self.get(key).await?.filter(|value| !value.is_null()) {
                Some(value) => self.put(key, &value, seconds).await,
                None => Ok(false),
            }
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move { self.disk.delete(&self.path(key)).await.map_err(backend) })
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(key, move |value| value.checked_add(amount)))
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(key, move |value| value.checked_sub(amount)))
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            let directory = (!self.directory.is_empty()).then_some(self.directory.as_str());
            if let Some(directory) = directory {
                match self.disk.deleteDirectory(directory).await {
                    Ok(true) => return self.disk.makeDirectory(directory).await.map_err(backend),
                    Ok(false) => return Ok(false),
                    Err(FilesystemError::UnsupportedOperation) => {}
                    Err(error) => return Err(backend(error)),
                }
            }
            let mut cleared = true;
            for path in self.disk.allFiles(directory).await.map_err(backend)? {
                cleared &= self.disk.delete(&path).await.map_err(backend)?;
            }
            Ok(cleared)
        })
    }

    fn get_prefix(&self) -> &str {
        self.getPrefix()
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        None
    }
}
