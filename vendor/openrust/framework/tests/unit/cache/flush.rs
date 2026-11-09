//! 清空具名缓存时的文件边界、内存代际和外部驱动能力回归。

use super::*;
use crate::foundation::lifecycle::BackgroundTasks;

fn store(backend: Arc<dyn CacheBackend>, namespace: &str) -> Result<CacheStore> {
    static TASKS: std::sync::OnceLock<BackgroundTasks> = std::sync::OnceLock::new();
    let tasks = TASKS.get_or_init(BackgroundTasks::default);
    Ok(CacheStore {
        store_name: "test".to_owned(),
        backend,
        namespace: namespace.to_owned(),
        singleflight: SingleFlight::new(&CacheSingleflightConfig::default())?,
        background_tasks: tasks.handle(),
        refresh_limiter: Arc::new(RefreshLimiter {
            active: AtomicUsize::new(0),
            maximum: 64,
        }),
        shared_locks: None,
        memo: None,
    })
}

#[tokio::test]
async fn flush_clears_a_file_store_without_removing_unowned_entries() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let backend: Arc<dyn CacheBackend> = Arc::new(FileBackend {
        root: root.path().to_owned(),
    });
    let first = store(Arc::clone(&backend), "first")?;
    let second = store(backend, "second")?;
    let directory = root.path().join("f".repeat(64));
    let unrelated = root.path().join("readme");
    let temp = root.path().join(".tmp-writer");
    std::fs::create_dir(&directory).map_err(Error::internal)?;
    std::fs::write(&unrelated, b"unrelated").map_err(Error::internal)?;
    std::fs::write(&temp, b"pending").map_err(Error::internal)?;
    #[cfg(unix)]
    let symlink = {
        let path = root.path().join("e".repeat(64));
        std::os::unix::fs::symlink(&unrelated, &path).map_err(Error::internal)?;
        path
    };

    first.set("value", &1, None).await?;
    second.set("value", &2, None).await?;
    first.flush().await?;
    first.flush().await?;
    assert_eq!(first.get::<u8>("value").await?, None);
    assert_eq!(second.get::<u8>("value").await?, None);
    assert!(directory.is_dir());
    assert!(unrelated.is_file());
    assert!(temp.is_file());
    #[cfg(unix)]
    assert!(symlink.is_symlink());
    Ok(())
}

#[tokio::test]
async fn memoized_flush_clears_the_selected_memory_backend() -> Result<()> {
    let backend: Arc<dyn CacheBackend> = Arc::new(MemoryBackend::default());
    let first = store(Arc::clone(&backend), "first")?;
    let mut memoized = store(backend, "second")?;
    memoized.memo = Some(Arc::default());
    first.set("value", &1, None).await?;
    memoized.set("value", &2, None).await?;
    assert_eq!(memoized.get::<u8>("value").await?, Some(2));

    let tagged = memoized.tagged(&["users"])?;
    let old_entry = tagged.entry("profile").await?;
    memoized.invalidate_tags(&["users"]).await?;
    memoized.flush().await?;
    assert!(
        old_entry
            .set(&"stale", Duration::from_secs(30))
            .await
            .is_err()
    );
    assert_eq!(memoized.get::<u8>("value").await?, None);
    assert_eq!(first.get::<u8>("value").await?, None);
    Ok(())
}

struct LegacyBackend;

#[async_trait]
impl CacheBackend for LegacyBackend {
    async fn get(&self, _: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn set(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<()> {
        Ok(())
    }

    async fn remove(&self, _: &str) -> Result<()> {
        Ok(())
    }

    fn scope(&self) -> CacheScope {
        CacheScope::Process
    }
}

#[tokio::test]
async fn flush_reports_an_unsupported_custom_driver() -> Result<()> {
    let cache = store(Arc::new(LegacyBackend), "legacy")?;
    let result = cache.flush().await;
    assert!(matches!(result, Err(error) if error.code() == "cache_flush_unsupported"));
    Ok(())
}
