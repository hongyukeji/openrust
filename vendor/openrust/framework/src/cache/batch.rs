use std::future::Future;

use indexmap::IndexMap;

use crate::contracts::cache::Repository as _;

use super::{Cache, CacheResult, Repository, Ttl, Value};

#[allow(non_snake_case, reason = "Matches Laravel's cache repository API")]
impl Repository {
    /// Reads a set of keys with a separate default for each key.
    pub async fn manyOr(&self, entries: &[(&str, Value)]) -> CacheResult<IndexMap<String, Value>> {
        let defaults: IndexMap<_, _> = entries.iter().map(|(key, value)| (*key, value)).collect();
        let keys: Vec<_> = defaults.keys().copied().collect();
        Ok(self
            .keyed_many(&keys)
            .await?
            .into_iter()
            .map(|(key, value)| {
                (
                    key.to_owned(),
                    value.unwrap_or_else(|| defaults[key].clone()),
                )
            })
            .collect())
    }

    /// Reads once and resolves defaults only for missing or null keys.
    pub async fn manyOrElse<'a, F>(
        &self,
        keys: &[&'a str],
        mut default: impl FnMut(&'a str) -> F,
    ) -> CacheResult<IndexMap<String, Value>>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        let entries = self.keyed_many(keys).await?;
        let mut results = IndexMap::with_capacity(entries.len());
        for (key, value) in entries {
            let value = match value {
                Some(value) => value,
                None => default(key).await?,
            };
            results.insert(key.to_owned(), value);
        }
        Ok(results)
    }

    /// Reads each key once, applying the same default to all misses.
    pub async fn getMultiple(
        &self,
        keys: &[&str],
        default: Option<Value>,
    ) -> CacheResult<IndexMap<String, Option<Value>>> {
        Ok(self
            .keyed_many(keys)
            .await?
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.or_else(|| default.clone())))
            .collect())
    }

    async fn keyed_many<'a>(
        &self,
        keys: &[&'a str],
    ) -> CacheResult<IndexMap<&'a str, Option<Value>>> {
        let mut entries: IndexMap<_, _> = keys.iter().map(|&key| (key, None)).collect();
        let keys: Vec<_> = entries.keys().copied().collect();
        let values = self.many(&keys).await?;
        for (entry, value) in entries.values_mut().zip(values) {
            *entry = value.filter(|value| !value.is_null());
        }
        Ok(entries)
    }

    pub async fn set(&self, key: &str, value: &Value, ttl: Option<Ttl>) -> CacheResult<bool> {
        self.put(key, value, ttl).await
    }

    pub async fn setMultiple(
        &self,
        values: &[(String, Value)],
        ttl: Option<Ttl>,
    ) -> CacheResult<bool> {
        self.putMany(values, ttl).await
    }

    pub async fn delete(&self, key: &str) -> CacheResult<bool> {
        self.forget(key).await
    }

    pub async fn deleteMultiple(&self, keys: &[&str]) -> CacheResult<bool> {
        let mut removed = true;
        for key in keys {
            removed &= self.forget(key).await?;
        }
        Ok(removed)
    }

    pub async fn clear(&self) -> CacheResult<bool> {
        self.flush().await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's cache facade API")]
impl Cache {
    pub async fn manyOr(entries: &[(&str, Value)]) -> CacheResult<IndexMap<String, Value>> {
        Self::store(None)?.manyOr(entries).await
    }

    pub async fn manyOrElse<'a, F>(
        keys: &[&'a str],
        default: impl FnMut(&'a str) -> F,
    ) -> CacheResult<IndexMap<String, Value>>
    where
        F: Future<Output = CacheResult<Value>>,
    {
        Self::store(None)?.manyOrElse(keys, default).await
    }

    pub async fn getMultiple(
        keys: &[&str],
        default: Option<Value>,
    ) -> CacheResult<IndexMap<String, Option<Value>>> {
        Self::store(None)?.getMultiple(keys, default).await
    }

    pub async fn set(key: &str, value: &Value, ttl: Option<Ttl>) -> CacheResult<bool> {
        Self::put(key, value, ttl).await
    }

    pub async fn setMultiple(values: &[(String, Value)], ttl: Option<Ttl>) -> CacheResult<bool> {
        Self::putMany(values, ttl).await
    }

    pub async fn delete(key: &str) -> CacheResult<bool> {
        Self::forget(key).await
    }

    pub async fn deleteMultiple(keys: &[&str]) -> CacheResult<bool> {
        Self::store(None)?.deleteMultiple(keys).await
    }

    pub async fn clear() -> CacheResult<bool> {
        Self::flush().await
    }
}
