//! 作用域缓存视图的命中、未命中与写入失效。

use std::{collections::HashMap, sync::Mutex};

use super::{CacheBackend, Result};

#[derive(Default)]
pub(super) struct MemoizedValues(Mutex<HashMap<String, Entry>>);

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
        let revision = {
            let mut entries = self.entries();
            let entry = entries.entry(key.to_owned()).or_default();
            if let Read::Stored(value) = &entry.read {
                return Ok(value.clone());
            }
            (entry.writes == 0).then_some(entry.revision)
        };
        let value = backend.get(key).await?;
        if let Some(entry) = self.entries().get_mut(key)
            && entry.writes == 0
            && revision == Some(entry.revision)
        {
            if let Read::Stored(cached) = &entry.read {
                return Ok(cached.clone());
            }
            entry.read = Read::Stored(value.clone());
        }
        Ok(value)
    }

    pub(super) fn begin_write<'a>(&'a self, key: &'a str) -> MemoWrite<'a> {
        let mut entries = self.entries();
        let entry = entries.entry(key.to_owned()).or_default();
        entry.read = Read::Unobserved;
        // 一个旧读取即使晚于写入完成，也不能重新填充这个作用域。
        entry.revision = entry.revision.wrapping_add(1);
        entry.writes += 1;
        MemoWrite { memo: self, key }
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
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
        if let Some(entry) = self.memo.entries().get_mut(self.key) {
            entry.writes -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CacheScope, MemoryBackend};
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };
    use tokio::sync::Notify;

    #[derive(Default)]
    struct PausedRead {
        memory: MemoryBackend,
        pause: AtomicBool,
        captured: Notify,
        resume: Notify,
    }

    #[async_trait::async_trait]
    impl CacheBackend for PausedRead {
        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            let value = self.memory.get(key).await?;
            if self.pause.swap(false, Ordering::Relaxed) {
                self.captured.notify_one();
                self.resume.notified().await;
            }
            Ok(value)
        }

        async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
            self.memory.set(key, value, ttl).await
        }

        async fn remove(&self, key: &str) -> Result<()> {
            self.memory.remove(key).await
        }

        fn scope(&self) -> CacheScope {
            self.memory.scope()
        }
    }

    #[tokio::test]
    async fn read_started_before_write_cannot_restore_old_value() -> Result<()> {
        let backend = PausedRead::default();
        backend.set("key", vec![1], None).await?;
        backend.pause.store(true, Ordering::Relaxed);
        let memo = MemoizedValues::default();

        let (first, write) = tokio::join!(memo.get(&backend, "key"), async {
            backend.captured.notified().await;
            let result = {
                let _write = memo.begin_write("key");
                backend.set("key", vec![2], None).await
            };
            backend.resume.notify_one();
            result
        });
        write?;
        assert_eq!(first?, Some(vec![1]));
        assert_eq!(memo.get(&backend, "key").await?, Some(vec![2]));

        backend.set("other", vec![3], None).await?;
        backend.pause.store(true, Ordering::Relaxed);
        let (other, write) = tokio::join!(memo.get(&backend, "other"), async {
            backend.captured.notified().await;
            let result = {
                let _write = memo.begin_write("key");
                backend.set("key", vec![4], None).await
            };
            backend.resume.notify_one();
            result
        });
        write?;
        assert_eq!(other?, Some(vec![3]));
        backend.set("other", vec![5], None).await?;
        assert_eq!(memo.get(&backend, "other").await?, Some(vec![3]));
        Ok(())
    }
}
