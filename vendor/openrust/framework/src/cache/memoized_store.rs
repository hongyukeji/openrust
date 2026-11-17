use std::{
    borrow::Cow,
    collections::HashMap,
    future::Future,
    num::NonZeroU64,
    sync::{Arc, Mutex, Weak},
};

use crate::contracts::cache::{CanFlushLocks, LockProvider, Repository as _, Store};

use super::{CacheFuture, CacheResult, Repository, TagSet, TaggedCache, Value};

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
    state: Arc<Mutex<MemoizedValues>>,
    lifetime: Option<Weak<()>>,
    tags: Option<Arc<TagSet>>,
}

#[derive(Default)]
struct MemoizedValues {
    plain: State,
    tagged: HashMap<Vec<String>, State>,
    plain_flushes: usize,
    tagged_flushes: usize,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    flush_epoch: u64,
}

impl State {
    fn invalidate(&mut self) {
        self.flush_epoch = self.flush_epoch.wrapping_add(1);
        self.entries.retain(|_, entry| {
            entry.read = Read::Unobserved;
            entry.writes != 0
        });
    }
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
        let store = Arc::new(Self {
            repository: Arc::clone(&repository),
            state: Arc::default(),
            lifetime,
            tags: None,
        });
        Arc::new(repository.memoized_view(store))
    }

    fn backend(&self) -> &dyn Store {
        self.repository.get_store()
    }

    pub(super) fn flush_memoized_tags(&self, names: &[String]) {
        if let Some(tags) = self.state().tagged.get_mut(names) {
            tags.invalidate();
        }
    }

    async fn memo_namespace(&self) -> CacheResult<Option<String>> {
        match &self.tags {
            // Laravel's RedisTagSet uses stable tag keys for memoized lookups.
            Some(tags) if tags.redis.is_none() => tags.getNamespace().await.map(Some),
            _ => Ok(None),
        }
    }

    fn observe(&self, key: &str) -> Observation {
        let mut values = self.state();
        if self.flushing(&values)
            || self
                .lifetime
                .as_ref()
                .is_some_and(|scope| scope.strong_count() == 0)
        {
            return Observation::Miss(None);
        }
        let state = self.store_state(&mut values);
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
        let mut values = self.state();
        if self.flushing(&values) {
            return value;
        }
        let state = self.store_state(&mut values);
        let epoch = state.flush_epoch;
        // Reads overlapping a write or flush cannot restore a stale entry.
        if let Some(entry) = state.entries.get_mut(key)
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

    fn begin_write<'a>(&'a self, key: impl Into<Cow<'a, str>>) -> Write<'a> {
        let key = key.into();
        let mut state = self.state();
        let state = self.store_state(&mut state);
        let entry = state.entries.entry(key.as_ref().to_owned()).or_default();
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
        let namespace = self.memo_namespace().await?;
        let _write = self.begin_write(memo_key(namespace.as_deref(), key));
        operation.await
    }

    fn begin_flush(&self) -> Flush<'_> {
        let mut state = self.state();
        if self.tags.is_none() {
            state.plain_flushes += 1;
            state.plain.invalidate();
        }
        state.tagged_flushes += 1;
        for tags in state.tagged.values_mut() {
            tags.invalidate();
        }
        Flush(self)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, MemoizedValues> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn store_state<'a>(&self, values: &'a mut MemoizedValues) -> &'a mut State {
        match &self.tags {
            Some(tags) => values
                .tagged
                .get_mut(tags.getNames())
                .expect("registered memoized tags"),
            None => &mut values.plain,
        }
    }

    fn flushing(&self, values: &MemoizedValues) -> bool {
        if self.tags.is_some() {
            values.tagged_flushes != 0
        } else {
            values.plain_flushes != 0
        }
    }
}

fn memo_key<'a>(namespace: Option<&str>, key: &'a str) -> Cow<'a, str> {
    match namespace {
        Some(namespace) => Cow::Owned(format!("{namespace}:{key}")),
        None => Cow::Borrowed(key),
    }
}

struct Write<'a> {
    store: &'a MemoizedStore,
    key: Cow<'a, str>,
}

impl Drop for Write<'_> {
    fn drop(&mut self) {
        let mut values = self.store.state();
        if let Some(entry) = self
            .store
            .store_state(&mut values)
            .entries
            .get_mut(self.key.as_ref())
        {
            entry.writes -= 1;
        }
    }
}

struct Flush<'a>(&'a MemoizedStore);

impl Drop for Flush<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state();
        state.tagged_flushes -= 1;
        if self.0.tags.is_none() {
            state.plain_flushes -= 1;
        }
    }
}

impl Store for MemoizedStore {
    fn tags(self: Arc<Self>, names: &[&str]) -> CacheResult<TaggedCache> {
        let mut tagged = self.repository.tags(names)?;
        self.state()
            .tagged
            .entry(tagged.tags.getNames().to_vec())
            .or_default();
        let store = Arc::new(Self {
            repository: Arc::new(tagged.repository.clone()),
            state: Arc::clone(&self.state),
            lifetime: self.lifetime.clone(),
            tags: Some(Arc::clone(&tagged.tags)),
        });
        tagged.repository = tagged.repository.memoized_view(store);
        tagged.memoized_parent = Some(self);
        Ok(tagged)
    }

    fn supports_tags(&self) -> bool {
        self.backend().supports_tags()
    }

    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            let namespace = self.memo_namespace().await?;
            let memo_key = memo_key(namespace.as_deref(), key);
            match self.observe(&memo_key) {
                Observation::Hit(value) => Ok(value),
                Observation::Miss(stamp) => {
                    Ok(self.record(&memo_key, stamp, self.repository.get(key).await?))
                }
            }
        })
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move {
            let namespace = self.memo_namespace().await?;
            let observations: Vec<_> = keys
                .iter()
                .map(|key| self.observe(&memo_key(namespace.as_deref(), key)))
                .collect();
            let missing: Vec<_> = keys
                .iter()
                .zip(&observations)
                .filter_map(|(&key, entry)| matches!(entry, Observation::Miss(_)).then_some(key))
                .collect();
            let mut values = if missing.is_empty() {
                Vec::new().into_iter()
            } else {
                self.repository.many(&missing).await?.into_iter()
            };
            Ok(keys
                .iter()
                .zip(observations)
                .map(|(key, entry)| match entry {
                    Observation::Hit(value) => value,
                    Observation::Miss(stamp) => self.record(
                        &memo_key(namespace.as_deref(), key),
                        stamp,
                        values.next().flatten(),
                    ),
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
        Box::pin(self.write(key, self.repository.put_seconds(key, value, Some(seconds))))
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let namespace = self.memo_namespace().await?;
            let _writes: Vec<_> = values
                .iter()
                .map(|(key, _)| self.begin_write(memo_key(namespace.as_deref(), key)))
                .collect();
            self.repository.put_many_seconds(values, seconds).await
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
        Box::pin(self.write(key, self.repository.pull(key)))
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.repository.forever(key, value)))
    }

    fn touch<'a>(
        &'a self,
        key: &'a str,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.backend().touch(key, seconds)))
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(self.write(key, self.repository.forget(key)))
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
            self.repository.flush().await
        })
    }

    fn get_prefix(&self) -> &str {
        self.backend().get_prefix()
    }

    fn lock_flusher(&self) -> Option<&dyn CanFlushLocks> {
        self.backend().lock_flusher()
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        self.backend().lock_provider()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/cache/memoized_store.rs"]
mod tests;
