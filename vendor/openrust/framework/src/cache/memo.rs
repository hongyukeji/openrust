//! 作用域缓存视图的命中、未命中与写入失效。

use std::{collections::HashMap, sync::Mutex};

use super::{CacheBackend, Result};

#[derive(Default)]
pub(super) struct MemoizedValues(Mutex<State>);

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
    Stored(Option<Vec<u8>>),
}

impl MemoizedValues {
    pub(super) async fn get(
        &self,
        backend: &dyn CacheBackend,
        key: &str,
    ) -> Result<Option<Vec<u8>>> {
        let observation = {
            let mut state = self.state();
            let epoch = state.flush_epoch;
            if state.flushes == 0 {
                let entry = state.entries.entry(key.to_owned()).or_default();
                if let Read::Stored(value) = &entry.read {
                    return Ok(value.clone());
                }
                (entry.writes == 0).then_some((epoch, entry.revision))
            } else {
                None
            }
        };
        let value = backend.get(key).await?;
        let mut state = self.state();
        let epoch = state.flush_epoch;
        if state.flushes == 0
            && let Some(entry) = state.entries.get_mut(key)
            && entry.writes == 0
            && observation == Some((epoch, entry.revision))
        {
            if let Read::Stored(cached) = &entry.read {
                return Ok(cached.clone());
            }
            entry.read = Read::Stored(value.clone());
        }
        Ok(value)
    }

    pub(super) fn begin_write<'a>(&'a self, key: &'a str) -> MemoWrite<'a> {
        let mut state = self.state();
        let entry = state.entries.entry(key.to_owned()).or_default();
        entry.read = Read::Unobserved;
        // 一个旧读取即使晚于写入完成，也不能重新填充这个作用域。
        entry.revision = entry.revision.wrapping_add(1);
        entry.writes += 1;
        MemoWrite { memo: self, key }
    }

    pub(super) fn begin_flush(&self) -> MemoFlush<'_> {
        let mut state = self.state();
        state.flush_epoch = state.flush_epoch.wrapping_add(1);
        state.flushes += 1;
        state.entries.retain(|_, entry| {
            entry.read = Read::Unobserved;
            entry.writes != 0
        });
        MemoFlush(self)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(super) struct MemoWrite<'a> {
    memo: &'a MemoizedValues,
    key: &'a str,
}

impl Drop for MemoWrite<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.memo.state().entries.get_mut(self.key) {
            entry.writes -= 1;
        }
    }
}

pub(super) struct MemoFlush<'a>(&'a MemoizedValues);

impl Drop for MemoFlush<'_> {
    fn drop(&mut self) {
        self.0.state().flushes -= 1;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/cache/memo.rs"]
mod tests;
