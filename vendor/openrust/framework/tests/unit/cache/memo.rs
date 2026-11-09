//! 缓存视图并发读取、写入与清理的回归。

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

#[tokio::test]
async fn flush_discards_hits_misses_and_overlapping_reads() -> Result<()> {
    let backend = PausedRead::default();
    let memo = MemoizedValues::default();
    backend.set("hit", vec![1], None).await?;
    backend.set("slow", vec![2], None).await?;
    assert_eq!(memo.get(&backend, "hit").await?, Some(vec![1]));
    assert_eq!(memo.get(&backend, "miss").await?, None);

    backend.pause.store(true, Ordering::Relaxed);
    let (slow, flushed) = tokio::join!(memo.get(&backend, "slow"), async {
        backend.captured.notified().await;
        let result: Result<()> = async {
            let _flush = memo.begin_flush();
            assert_eq!(memo.get(&backend, "slow").await?, Some(vec![2]));
            backend.memory.flush().await?;
            backend.set("miss", vec![3], None).await
        }
        .await;
        backend.resume.notify_one();
        result
    });

    flushed?;
    assert_eq!(slow?, Some(vec![2]));
    assert_eq!(memo.get(&backend, "slow").await?, None);
    assert_eq!(memo.get(&backend, "hit").await?, None);
    assert_eq!(memo.get(&backend, "miss").await?, Some(vec![3]));
    Ok(())
}
