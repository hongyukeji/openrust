use std::{
    collections::HashMap,
    future::Future,
    num::NonZeroU64,
    sync::{Arc, Mutex, Weak},
};

use crate::contracts::cache::{LockProvider, Repository as _, Store};

use super::{CacheFuture, CacheResult, Repository, Value};

#[derive(Default)]
pub(super) struct MemoizedRepositories {
    lifetime: Arc<()>,
    stores: Mutex<HashMap<usize, Arc<Repository>>>,
}

impl MemoizedRepositories {
    pub(super) fn repository(&self, repository: Arc<Repository>) -> Arc<Repository> {
        let identity = Arc::as_ptr(&repository) as usize;
        self.stores
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(identity)
            .or_insert_with(|| {
                MemoizedStore::wrap(repository, Some(Arc::downgrade(&self.lifetime)))
            })
            .clone()
    }
}

pub(super) struct MemoizedStore {
    repository: Arc<Repository>,
    state: Mutex<State>,
    lifetime: Option<Weak<()>>,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    flush_epoch: u64,
    flushes: usize,
}

#[derive(Default)]
struct Entry {
    read: Read,
    revision: u64,
    writes: usize,
}

#[derive(Default)]
enum Read {
    #[default]
    Unobserved,
    Stored(Option<Value>),
}

enum Observation {
    Hit(Option<Value>),
    Miss(Option<(u64, u64)>),
}

impl MemoizedStore {
    pub(super) fn wrap(repository: Arc<Repository>, lifetime: Option<Weak<()>>) -> Arc<Repository> {
        Arc::new(Repository::new(Arc::new(Self {
            repository,
            state: Mutex::default(),
            lifetime,
        })))
    }

    fn backend(&self) -> &dyn Store {
        self.repository.get_store()
    }

    fn observe(&self, key: &str) -> Observation {
        let mut state = self.state();
        if state.flushes != 0
            || self
                .lifetime
                .as_ref()
                .is_some_and(|scope| scope.strong_count() == 0)
        {
            return Observation::Miss(None);
        }
        let epoch = state.flush_epoch;
        let entry = match state.entries.get_mut(key) {
            Some(entry) => entry,
            None => state.entries.entry(key.to_owned()).or_default(),
        };
        if let Read::Stored(value) = &entry.read {
            return Observation::Hit(value.clone());
        }
        Observation::Miss((entry.writes == 0).then_some((epoch, entry.revision)))
    }

    fn record(
        &self,
        key: &str,
        observation: Option<(u64, u64)>,
        value: Option<Value>,
    ) -> Option<Value> {
        let Some(observation) = observation else {
            return value;
        };
        let mut state = self.state();
        let epoch = state.flush_epoch;
        // Reads overlapping a write or flush cannot restore a stale entry.
        if state.flushes == 0
            && let Some(entry) = state.entries.get_mut(key)
            && entry.writes == 0
            && observation == (epoch, entry.revision)
        {
            if let Read::Stored(cached) = &entry.read {
                return cached.clone();
            }
            entry.read = Read::Stored(value.clone());
        }
        value
    }

    fn begin_write<'a>(&'a self, key: &'a str) -> Write<'a> {
        let mut state = self.state();
        let entry = state.entries.entry(key.to_owned()).or_default();
        entry.read = Read::Unobserved;
        entry.revision = entry.revision.wrapping_add(1);
        entry.writes += 1;
        Write { store: self, key }
    }

    async fn write<T>(
        &self,
        key: &str,
        operation: impl Future<Output = CacheResult<T>>,
    ) -> CacheResult<T> {
        let _write = self.begin_write(key);
        operation.await
    }

    fn begin_flush(&self) -> Flush<'_> {
        let mut state = self.state();
        state.flush_epoch = state.flush_epoch.wrapping_add(1);
        state.flushes += 1;
        state.entries.retain(|_, entry| {
            entry.read = Read::Unobserved;
            entry.writes != 0
        });
        Flush(self)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

struct Write<'a> {
    store: &'a MemoizedStore,
    key: &'a str,
}

impl Drop for Write<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.store.state().entries.get_mut(self.key) {
            entry.writes -= 1;
        }
    }
}

struct Flush<'a>(&'a MemoizedStore);

impl Drop for Flush<'_> {
    fn drop(&mut self) {
        self.0.state().flushes -= 1;
    }
}

impl Store for MemoizedStore {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            match self.observe(key) {
                Observation::Hit(value) => Ok(value),
                Observation::Miss(stamp) => {
                    Ok(self.record(key, stamp, self.backend().get(key).await?))
                }
            }
        })
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move {
            let observations: Vec<_> = keys.iter().map(|key| self.observe(key)).collect();
            let missing: Vec<_> = keys
                .iter()
                .zip(&observations)
                .filter_map(|(&key, entry)| matches!(entry, Observation::Miss(_)).then_some(key))
                .collect();
            let mut values = if missing.is_empty() {
                Vec::new().into_iter()
            } else {
                self.backend().many(&missing).await?.into_iter()
            };
            Ok(keys
                .iter()
                .zip(observations)
                .map(|(key, entry)| match entry {
                    Observation::Hit(value) => value,
                    Observation::Miss(stamp) => self.record(key, stamp, values.next().flatten()),
                })
                .collect())
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.backend().put(key, value, seconds)))
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let _writes: Vec<_> = values
                .iter()
                .map(|(key, _)| self.begin_write(key))
                .collect();
            self.backend().put_many(values, seconds).await
        })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.backend().add(key, value, seconds)))
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(self.write(key, self.backend().pull(key)))
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.backend().forever(key, value)))
    }

    fn touch<'a>(
        &'a self,
        key: &'a str,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.backend().touch(key, seconds)))
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.backend().forget(key)))
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.write(key, self.backend().increment(key, amount)))
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.write(key, self.backend().decrement(key, amount)))
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            let _flush = self.begin_flush();
            self.backend().flush().await
        })
    }

    fn get_prefix(&self) -> &str {
        self.backend().get_prefix()
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        self.backend().lock_provider()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/cache/memoized_store.rs"]
mod tests;
