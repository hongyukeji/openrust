use std::{num::NonZeroU64, time::Duration};

use crate::contracts::cache::{Lock as LockContract, LockProvider, Store};

use super::{
    CacheError, CacheFuture, CacheResult, Lock, Value,
    engine::{Change, Engine, FOREVER, Record, expires},
};

pub(super) struct StoreCore {
    data: Engine,
    locks: Engine,
    prefix: String,
    database: bool,
}

impl StoreCore {
    pub(super) fn new(data: Engine, locks: Engine, prefix: String, database: bool) -> Self {
        Self {
            data,
            locks,
            prefix,
            database,
        }
    }

    async fn write(
        &self,
        key: &str,
        value: &Value,
        seconds: Option<NonZeroU64>,
        add: bool,
    ) -> CacheResult<bool> {
        let value = value.clone();
        self.data
            .change(self.key(key), move |entry| {
                if add && entry.is_some_and(|entry| seconds.is_some() || !entry.value.is_null()) {
                    return Ok((Change::Keep, false));
                }
                let expires = expires(seconds.map(|seconds| Duration::from_secs(seconds.get())))?;
                Ok((
                    Change::Write(Record {
                        value: value.clone(),
                        expires,
                    }),
                    true,
                ))
            })
            .await
    }

    fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }
}

impl Store for StoreCore {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            Ok(self
                .data
                .read(self.key(key))
                .await?
                .map(|entry| entry.value))
        })
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
        Box::pin(self.write(key, value, Some(seconds), false))
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            for (key, value) in values {
                self.put(key, value, seconds).await?;
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
        Box::pin(self.write(key, value, seconds, true))
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, value, None, false))
    }

    fn touch<'a>(
        &'a self,
        key: &'a str,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.data.change(self.key(key), move |entry| {
            let Some(mut entry) = entry else {
                return Ok((Change::Keep, false));
            };
            entry.expires = expires(Some(Duration::from_secs(seconds.get())))?;
            Ok((Change::Write(entry), true))
        }))
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(self.data.change(self.key(key), |entry| {
            Ok((Change::Remove, entry.map(|entry| entry.value)))
        }))
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move { Ok(self.pull(key).await?.is_some()) })
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        let database = self.database;
        Box::pin(self.data.change(self.key(key), move |entry| {
            let (current, expires) = match entry {
                Some(entry) if !entry.value.is_null() => (
                    entry.value.as_i64().ok_or(CacheError::InvalidValue)?,
                    entry.expires,
                ),
                _ if database => return Err(CacheError::MissingCounter),
                _ => (0, FOREVER),
            };
            let value = current
                .checked_add(amount)
                .ok_or(CacheError::InvalidValue)?;
            Ok((
                Change::Write(Record {
                    value: value.into(),
                    expires,
                }),
                value,
            ))
        }))
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(async move {
            self.increment(key, amount.checked_neg().ok_or(CacheError::InvalidValue)?)
                .await
        })
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move { self.data.flush().await.map(|()| true) })
    }

    fn get_prefix(&self) -> &str {
        &self.prefix
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        Some(self)
    }
}

impl LockProvider for StoreCore {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn LockContract>> {
        Ok(Box::new(Lock::new(
            self.locks.clone(),
            self.key(name),
            lease,
            owner,
            self.database,
        )?))
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn LockContract>> {
        self.lock(name, None, Some(owner))
    }
}

macro_rules! impl_store {
    ($store:ty) => {
        impl $crate::contracts::cache::Store for $store {
            fn get<'a>(
                &'a self,
                key: &'a str,
            ) -> $crate::cache::CacheFuture<
                'a,
                $crate::cache::CacheResult<Option<$crate::cache::Value>>,
            > {
                $crate::contracts::cache::Store::get(&self.0, key)
            }

            fn many<'a>(
                &'a self,
                keys: &'a [&'a str],
            ) -> $crate::cache::CacheFuture<
                'a,
                $crate::cache::CacheResult<Vec<Option<$crate::cache::Value>>>,
            > {
                $crate::contracts::cache::Store::many(&self.0, keys)
            }

            fn put<'a>(
                &'a self,
                key: &'a str,
                value: &'a $crate::cache::Value,
                seconds: std::num::NonZeroU64,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<bool>> {
                $crate::contracts::cache::Store::put(&self.0, key, value, seconds)
            }

            fn put_many<'a>(
                &'a self,
                values: &'a [(String, $crate::cache::Value)],
                seconds: std::num::NonZeroU64,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<bool>> {
                $crate::contracts::cache::Store::put_many(&self.0, values, seconds)
            }

            fn add<'a>(
                &'a self,
                key: &'a str,
                value: &'a $crate::cache::Value,
                seconds: Option<std::num::NonZeroU64>,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<bool>> {
                $crate::contracts::cache::Store::add(&self.0, key, value, seconds)
            }

            fn pull<'a>(
                &'a self,
                key: &'a str,
            ) -> $crate::cache::CacheFuture<
                'a,
                $crate::cache::CacheResult<Option<$crate::cache::Value>>,
            > {
                $crate::contracts::cache::Store::pull(&self.0, key)
            }

            fn forever<'a>(
                &'a self,
                key: &'a str,
                value: &'a $crate::cache::Value,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<bool>> {
                $crate::contracts::cache::Store::forever(&self.0, key, value)
            }

            fn touch<'a>(
                &'a self,
                key: &'a str,
                seconds: std::num::NonZeroU64,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<bool>> {
                $crate::contracts::cache::Store::touch(&self.0, key, seconds)
            }

            fn forget<'a>(
                &'a self,
                key: &'a str,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<bool>> {
                $crate::contracts::cache::Store::forget(&self.0, key)
            }

            fn increment<'a>(
                &'a self,
                key: &'a str,
                amount: i64,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<i64>> {
                $crate::contracts::cache::Store::increment(&self.0, key, amount)
            }

            fn decrement<'a>(
                &'a self,
                key: &'a str,
                amount: i64,
            ) -> $crate::cache::CacheFuture<'a, $crate::cache::CacheResult<i64>> {
                $crate::contracts::cache::Store::decrement(&self.0, key, amount)
            }

            fn flush(&self) -> $crate::cache::CacheFuture<'_, $crate::cache::CacheResult<bool>> {
                $crate::contracts::cache::Store::flush(&self.0)
            }

            fn get_prefix(&self) -> &str {
                $crate::contracts::cache::Store::get_prefix(&self.0)
            }

            fn lock_provider(&self) -> Option<&dyn $crate::contracts::cache::LockProvider> {
                $crate::contracts::cache::Store::lock_provider(&self.0)
            }
        }
    };
}

pub(super) use impl_store;
