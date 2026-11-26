//! 外部实现仅使用公开契约；具名解析及所有存储入口不依赖框架私有类型。
use openrust::{
    config::Repository,
    filesystem::{FilesystemAdapter, FilesystemError, FilesystemManager, Visibility},
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::SystemTime,
};

#[path = "../../../tests/support/driver_extensions/storage.rs"]
mod external;
use external::External;

fn manager(root: &std::path::Path) -> FilesystemManager {
    let repo = Repository::new(std::collections::BTreeMap::from([(
        "filesystems".into(),
        json!({"default":"remote", "disks":{
         "base":{"driver":"local", "root":"base", "throw":true},
         "remote":{"driver":"external", "root":"ignored", "serve":true},
         "other":{"driver":"external", "backing":"other_base", "url":"https://cdn.example/assets"},
            "other_base":{"driver":"local", "root":"other", "throw":true}
        }}),
    )]));
    FilesystemManager::new(Arc::new(repo), root.into())
}
fn register(manager: &FilesystemManager, writes: Arc<AtomicUsize>, builds: Arc<AtomicUsize>) {
    manager.extend("external", move |manager, config| {
        builds.fetch_add(1, Ordering::SeqCst);
        // 工厂解析另一磁盘，验证管理器没有持有全局缓存锁。
        let disk = manager.disk(Some(config["backing"].as_str().unwrap_or("base")))?;
        Ok(Arc::new(FilesystemAdapter::custom(
            Arc::new(External {
                disk,
                writes: writes.clone(),
            }),
            true,
            false,
            config["url"].as_str().map(str::to_owned),
        )))
    });
}
#[tokio::test]
async fn external_backend_streams_metadata_paths_and_operations_use_public_contracts() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let writes = Arc::new(AtomicUsize::new(0));
    let builds = Arc::new(AtomicUsize::new(0));
    register(&manager, writes.clone(), builds.clone());
    let disk = manager.disk(None).unwrap();
    assert!(Arc::ptr_eq(&disk, &manager.disk(None).unwrap()));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(disk.path("a.txt").unwrap().to_str(), Some("external/a.txt"));
    assert_eq!(disk.url("a.txt").unwrap(), "https://external.example/a.txt");
    let data = vec![b'a'; 200_003];
    assert!(disk.put("folder///a.txt", &data).await.unwrap());
    assert_eq!(disk.get("folder/a.txt").await.unwrap().unwrap(), data);
    assert_eq!(disk.size("folder/a.txt").await.unwrap(), 200_003);
    assert!(disk.last_modified("folder/a.txt").await.is_ok());
    assert_eq!(
        disk.mime_type("folder/a.txt").await.unwrap().as_deref(),
        Some("text/plain")
    );
    assert_eq!(
        disk.checksum("folder/a.txt").await.unwrap().unwrap(),
        "50ff0764172d0bc5fc75874abe30dfbb"
    );
    assert!(disk.file_exists("folder/a.txt").await.unwrap());
    assert!(!disk.directory_exists("folder/a.txt").await.unwrap());
    assert!(disk.directory_exists("folder").await.unwrap());
    assert!(disk.copy("folder/a.txt", "folder/b.txt").await.unwrap());
    assert!(disk.r#move("folder/b.txt", "folder/c.txt").await.unwrap());
    assert_eq!(
        disk.all_files(Some("folder")).await.unwrap(),
        vec!["folder/a.txt", "folder/c.txt"]
    );
    assert_eq!(disk.directories(None).await.unwrap(), vec!["folder"]);
    assert!(disk.make_directory("empty").await.unwrap());
    assert!(disk.delete_directory("empty").await.unwrap());
    assert!(
        disk.set_visibility("folder/a.txt", Visibility::Public)
            .await
            .unwrap()
    );
    assert_eq!(
        disk.get_visibility("folder/a.txt").await.unwrap(),
        Visibility::Public
    );
    let response = disk
        .download("folder/a.txt", None, Default::default())
        .await
        .unwrap();
    assert_eq!(
        axum::body::to_bytes(response.into_parts().1, 300_000)
            .await
            .unwrap()
            .as_ref(),
        data
    );
    for path in ["../x", "/x", "x/../y", "x\\y", "x/.storage-write-bad", ""] {
        assert!(matches!(
            disk.put(path, b"x").await,
            Err(FilesystemError::InvalidPath)
        ));
    }
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(disk.delete("folder/a.txt").await.unwrap());
    assert!(disk.delete_directory("folder").await.unwrap());
    assert!(disk.files(None).await.unwrap().is_empty());
    assert!(!disk.provides_temporary_urls());
    disk.build_temporary_urls_using(|path, _, _| {
        Box::pin(async move { Ok(format!("https://signed.example/{path}")) })
    });
    assert!(disk.provides_temporary_urls());
    assert_eq!(
        disk.temporary_url("a.txt", SystemTime::now())
            .await
            .unwrap(),
        "https://signed.example/a.txt"
    );
    assert_eq!(
        manager.disk(Some("other")).unwrap().url("a.txt").unwrap(),
        "https://cdn.example/assets/a.txt"
    );
    let other = manager.disk(Some("other")).unwrap();
    other.put("profile-only", b"other-profile").await.unwrap();
    assert!(matches!(
        disk.get("profile-only").await,
        Err(FilesystemError::NotFound)
    ));
    assert_eq!(
        other.get("profile-only").await.unwrap().unwrap(),
        b"other-profile"
    );
}

#[test]
fn failed_factory_retry_replacement_cache_and_application_isolation() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    assert!(matches!(
        manager.disk(None),
        Err(FilesystemError::UnsupportedOperation)
    ));
    manager.extend("external", |_, _| {
        Err(FilesystemError::InvalidConfiguration)
    });
    assert!(matches!(
        manager.disk(None),
        Err(FilesystemError::InvalidConfiguration)
    ));
    register(&manager, Arc::default(), Arc::default());
    let old = manager.disk(None).unwrap();
    manager.extend("external", |manager, _| manager.disk(Some("base")));
    assert!(Arc::ptr_eq(&old, &manager.disk(None).unwrap()));
    manager.forget_disk("remote");
    assert!(!Arc::ptr_eq(&old, &manager.disk(None).unwrap()));
    assert!(matches!(
        self::manager(root.path()).disk(None),
        Err(FilesystemError::UnsupportedOperation)
    ));
    let a = manager
        .build_from_config(json!({"driver":"external"}))
        .unwrap();
    assert!(Arc::ptr_eq(&a, &manager.disk(Some("base")).unwrap()));
    assert!(matches!(
        manager.build_from_config(json!({})),
        Err(FilesystemError::InvalidConfiguration)
    ));
}

#[tokio::test]
async fn external_stream_failure_early_return_and_error_policy_do_not_publish_partial_data() {
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{AsyncRead, ReadBuf};
    struct Pending;
    impl AsyncRead for Pending {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }
    struct Failed;
    impl AsyncRead for Failed {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::other("源数据读取失败")))
        }
    }
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    register(&manager, Arc::default(), Arc::default());
    let disk = manager.disk(None).unwrap();
    disk.put("existing", b"original").await.unwrap();
    assert!(disk.write_stream("existing", &mut Failed).await.is_err());
    assert_eq!(disk.get("existing").await.unwrap().unwrap(), b"original");
    assert!(matches!(
        disk.get("absent").await,
        Err(FilesystemError::NotFound)
    ));
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        disk.write_stream("early", &mut Pending),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(FilesystemError::UnsupportedOperation)));
    let data = vec![1; 400_000];
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), disk.put("early", &data))
        .await
        .unwrap();
    assert!(matches!(result, Err(FilesystemError::UnsupportedOperation)));
    let quiet = FilesystemAdapter::custom(manager.disk(Some("base")).unwrap(), false, false, None);
    assert!(quiet.get("absent").await.unwrap().is_none());
    assert!(!quiet.write_stream("absent", &mut Failed).await.unwrap());
    assert!(matches!(
        quiet.put("../outside", b"x").await,
        Err(FilesystemError::InvalidPath)
    ));
}

#[test]
fn concurrent_resolution_builds_only_once_per_disk() {
    let root = tempfile::tempdir().unwrap();
    let manager = Arc::new(manager(root.path()));
    let builds = Arc::new(AtomicUsize::new(0));
    register(&manager, Arc::default(), builds.clone());
    let barrier = std::sync::Barrier::new(12);
    std::thread::scope(|scope| {
        for _ in 0..12 {
            let barrier = &barrier;
            let manager = &manager;
            scope.spawn(move || {
                barrier.wait();
                manager.disk(None).unwrap();
            });
        }
    });
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}
