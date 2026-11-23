mod support;
use openrust::{
    cache::{ArrayStore, RedisStore, Repository as Cache},
    config::Repository,
    contracts::config::Repository as _,
    database::{DatabaseManager, Transaction},
    redis::RedisManager,
};
use openrust_settings::{
    Settings, SettingsError as Error, SettingsFuture, SettingsManager, SettingsNotification,
    SettingsOptions, SettingsRepository, SettingsSnapshot, SettingsUpdate,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::{Fixture, Site};

fn options(namespace: &str) -> SettingsOptions {
    SettingsOptions {
        namespace: namespace.into(),
        refresh_interval: Duration::from_millis(50),
        ..Default::default()
    }
}
fn cache() -> Arc<Cache> {
    if let Ok(url) = std::env::var("SETTINGS_TEST_REDIS_URL") {
        let cfg = Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"redis":{"default":{"url":url,"max_retries":0}}}),
        )]));
        let redis = RedisManager::new(Arc::new(cfg));
        let connection = redis.connection(None).unwrap();
        Arc::new(Cache::new(Arc::new(RedisStore::new(
            connection.clone(),
            connection,
            "settings-sync:".into(),
        ))))
    } else {
        Arc::new(Cache::new(Arc::new(ArrayStore::new())))
    }
}
async fn settings(manager: &SettingsManager, f: &Fixture) -> Settings<Site> {
    Settings::load(Arc::new(manager.clone()), &f.config, None)
        .await
        .unwrap()
}
async fn until(mut check: impl AsyncFnMut() -> bool) {
    let end = tokio::time::Instant::now() + Duration::from_secs(10);
    while !check().await {
        assert!(
            tokio::time::Instant::now() < end,
            "settings did not converge"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
#[tokio::test]
async fn same_repository_with_or_without_cache_refreshes_new_scopes_and_preserves_edits() {
    for cached in [false, true] {
        let f = Fixture::new().await;
        let cache = cached.then(cache);
        let first =
            SettingsManager::new(f.repository.clone(), options(&f.table), cache.clone()).unwrap();
        let second = SettingsManager::new(f.repository.clone(), options(&f.table), cache).unwrap();
        let old = settings(&second, &f).await;
        old.fill(json!({"count":9})).await.unwrap();
        let writer = settings(&first, &f).await;
        writer.fill(json!({"name":"new"})).await.unwrap();
        writer.save().await.unwrap();
        until(async || settings(&second, &f).await.get().await.name == "new").await;
        assert_eq!(old.get().await.name, "Default");
        assert_eq!(old.get().await.count, 9);
        assert!(matches!(old.save().await, Err(Error::Conflict)));
        first.shutdown().await;
        second.shutdown().await;
        f.close().await;
    }
}
#[tokio::test]
async fn notifications_are_hints_duplicates_and_late_versions_do_not_repeat_or_downgrade() {
    let f = Fixture::new().await;
    let manager =
        SettingsManager::new(f.repository.clone(), options(&f.table), Some(cache())).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    manager
        .on_change("site", move |change| {
            let c = c.clone();
            Box::pin(async move {
                assert_eq!(change.group, "site");
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .await
        .unwrap();
    settings(&manager, &f).await;
    let editor = f.site().await;
    editor.fill(json!({"name":"one"})).await.unwrap();
    editor.save().await.unwrap();
    manager.poll().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let first = f
        .repository
        .load("site")
        .await
        .unwrap()
        .version()
        .map(str::to_owned);
    let notice = SettingsNotification {
        namespace: f.table.clone(),
        group: "site".into(),
        version: first.clone(),
    };
    for _ in 0..10 {
        assert!(!manager.notify(&notice).await.unwrap());
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let editor = f.site().await;
    editor.fill(json!({"name":"two"})).await.unwrap();
    editor.save().await.unwrap();
    manager.poll().await.unwrap();
    assert!(!manager.notify(&notice).await.unwrap());
    assert_eq!(settings(&manager, &f).await.get().await.name, "two");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let mut foreign = notice;
    foreign.namespace = "another-app".into();
    assert!(!manager.notify(&foreign).await.unwrap());
    assert_eq!(count.load(Ordering::SeqCst), 2);
    manager.shutdown().await;
    f.close().await;
}
#[tokio::test]
async fn rollback_is_not_published_and_outer_commit_refreshes_without_discarding_editor_state() {
    let f = Fixture::new().await;
    let manager =
        SettingsManager::new(f.repository.clone(), options(&f.table), Some(cache())).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    manager
        .on_change("site", move |_| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .await
        .unwrap();
    let editor = settings(&manager, &f).await;
    editor.fill(json!({"name":"committed"})).await.unwrap();
    let mut tx = f.connection.begin_transaction().await.unwrap();
    editor.save_in(&mut tx).await.unwrap();
    tx.roll_back(None).await.unwrap();
    drop(tx);
    manager.poll().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(settings(&manager, &f).await.get().await.name, "Default");
    let mut tx = f.connection.begin_transaction().await.unwrap();
    editor.save_in(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    drop(tx);
    until(async || {
        manager.poll().await.unwrap();
        count.load(Ordering::SeqCst) == 1
    })
    .await;
    assert_eq!(settings(&manager, &f).await.get().await.name, "committed");
    // 保存中的旧实例仍由调用者显式刷新，不在后台覆盖它。
    editor.refresh().await.unwrap();
    assert_eq!(editor.get().await.name, "committed");
    manager.shutdown().await;
    f.close().await;
}
#[tokio::test]
async fn failing_handlers_retry_and_reentrant_reads_do_not_deadlock() {
    let f = Fixture::new().await;
    let manager = SettingsManager::new(f.repository.clone(), options(&f.table), None).unwrap();
    let allowed = Arc::new(AtomicBool::new(false));
    let a = allowed.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let reentrant = manager.downgrade();
    let handler = manager
        .on_change("site", move |_| {
            let a = a.clone();
            let c = c.clone();
            let m = reentrant.clone();
            Box::pin(async move {
                m.upgrade().ok_or(Error::Refresh)?.load("site").await?;
                if !a.load(Ordering::SeqCst) {
                    return Err(Error::Refresh);
                }
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .await
        .unwrap();
    settings(&manager, &f).await;
    let editor = f.site().await;
    editor.fill(json!({"name":"pending"})).await.unwrap();
    editor.save().await.unwrap();
    manager.poll().await.unwrap();
    let state = manager.status("site").await.unwrap();
    assert_eq!(state.pending_handlers, 1);
    assert_eq!(state.last_error, Some(Error::Refresh));
    allowed.store(true, Ordering::SeqCst);
    manager.poll().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(manager.status("site").await.unwrap().pending_handlers, 0);
    manager.poll().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(manager.forget_handler("site", handler).await);
    manager.shutdown().await;
    f.close().await;
}
struct Unavailable {
    source: Arc<dyn SettingsRepository>,
    offline: AtomicBool,
    versions: AtomicUsize,
    loads: AtomicUsize,
}
impl SettingsRepository for Unavailable {
    fn load<'a>(&'a self, g: &'a str) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            if self.offline.load(Ordering::SeqCst) {
                return Err(Error::Database);
            }
            self.loads.fetch_add(1, Ordering::SeqCst);
            self.source.load(g).await
        })
    }
    fn versions<'a>(
        &'a self,
        g: &'a [String],
    ) -> SettingsFuture<'a, BTreeMap<String, Option<String>>> {
        Box::pin(async move {
            if self.offline.load(Ordering::SeqCst) {
                return Err(Error::Database);
            }
            self.versions.fetch_add(1, Ordering::SeqCst);
            self.source.versions(g).await
        })
    }
    fn save<'a>(
        &'a self,
        g: &'a str,
        e: &'a SettingsSnapshot,
        c: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot> {
        self.source.save(g, e, c)
    }
    fn save_in<'a>(
        &'a self,
        t: &'a mut Transaction,
        g: &'a str,
        e: &'a SettingsSnapshot,
        c: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot> {
        self.source.save_in(t, g, e, c)
    }
}
#[tokio::test]
async fn source_failure_preserves_previous_scope_and_recovers_latest_version() {
    let f = Fixture::new().await;
    let source = Arc::new(Unavailable {
        source: f.repository.clone(),
        offline: AtomicBool::new(false),
        versions: AtomicUsize::new(0),
        loads: AtomicUsize::new(0),
    });
    let manager = SettingsManager::new(source.clone(), options(&f.table), None).unwrap();
    let old = settings(&manager, &f).await;
    source.offline.store(true, Ordering::SeqCst);
    assert!(manager.poll().await.is_err());
    assert_eq!(
        manager.status("site").await.unwrap().last_error,
        Some(Error::Database)
    );
    assert!(manager.load("site").await.is_err());
    assert_eq!(old.get().await.name, "Default");
    let writer = f.site().await;
    writer.fill(json!({"name":"offline change"})).await.unwrap();
    writer.save().await.unwrap();
    source.offline.store(false, Ordering::SeqCst);
    manager.poll().await.unwrap();
    assert_eq!(
        settings(&manager, &f).await.get().await.name,
        "offline change"
    );
    let before = source.loads.load(Ordering::SeqCst);
    manager.poll().await.unwrap();
    assert_eq!(source.loads.load(Ordering::SeqCst), before);
    f.connection.disconnect();
    manager.poll().await.unwrap();
    assert_eq!(
        settings(&manager, &f).await.get().await.name,
        "offline change"
    );
    manager.shutdown().await;
    f.close().await;
}
#[tokio::test]
async fn applications_using_the_same_cache_remain_isolated() {
    let first = Fixture::new().await;
    let second = Fixture::new().await;
    let shared = cache();
    let a = SettingsManager::new(
        first.repository.clone(),
        options(&first.table),
        Some(shared.clone()),
    )
    .unwrap();
    let b = SettingsManager::new(
        second.repository.clone(),
        options(&second.table),
        Some(shared),
    )
    .unwrap();
    settings(&a, &first).await;
    settings(&b, &second).await;
    let writer = settings(&a, &first).await;
    writer.fill(json!({"name":"app A"})).await.unwrap();
    writer.save().await.unwrap();
    b.poll().await.unwrap();
    assert_eq!(settings(&b, &second).await.get().await.name, "Default");
    let notice = SettingsNotification {
        namespace: first.table.clone(),
        group: "site".into(),
        version: a.status("site").await.unwrap().observed_version,
    };
    assert!(!b.notify(&notice).await.unwrap());
    a.shutdown().await;
    b.shutdown().await;
    first.close().await;
    second.close().await;
}
#[tokio::test]
#[ignore = "需要 Settings 专属临时 Redis 容器，不能使用开发实例"]
async fn redis_disconnect_flush_and_recovery_do_not_lose_committed_settings() {
    let container = std::env::var("SETTINGS_TEST_REDIS_CONTAINER").unwrap();
    assert!(container.starts_with("settings-sync-redis-"));
    let label = std::process::Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{index .Config.Labels \"org.openrust.settings-sync\"}}",
            &container,
        ])
        .output()
        .unwrap();
    assert!(label.status.success());
    assert_eq!(
        String::from_utf8(label.stdout).unwrap().trim(),
        std::env::var("SETTINGS_TEST_RUN_ID").unwrap()
    );
    let f = Fixture::new().await;
    let shared = cache();
    let manager = SettingsManager::new(
        f.repository.clone(),
        options(&f.table),
        Some(shared.clone()),
    )
    .unwrap();
    settings(&manager, &f).await;
    assert!(
        std::process::Command::new("docker")
            .args(["pause", &container])
            .status()
            .unwrap()
            .success()
    );
    struct Resume(String);
    impl Drop for Resume {
        fn drop(&mut self) {
            let _ = std::process::Command::new("docker")
                .args(["unpause", &self.0])
                .output();
        }
    }
    let resume = Resume(container);
    let writer = f.site().await;
    writer
        .fill(json!({"name":"during Redis outage"}))
        .await
        .unwrap();
    writer.save().await.unwrap();
    manager.poll().await.unwrap();
    assert!(manager.cache_failures() > 0);
    assert_eq!(
        settings(&manager, &f).await.get().await.name,
        "during Redis outage"
    );
    drop(resume);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    shared.get_store().flush().await.unwrap();
    manager.poll().await.unwrap();
    let writer = f.site().await;
    writer.fill(json!({"name":"after recovery"})).await.unwrap();
    writer.save().await.unwrap();
    manager.poll().await.unwrap();
    assert_eq!(
        settings(&manager, &f).await.get().await.name,
        "after recovery"
    );
    manager.shutdown().await;
    f.close().await;
}

#[tokio::test]
async fn explicit_refresh_bypasses_the_interval_and_shutdown_releases_weak_owner() {
    let f = Fixture::new().await;
    let manager = SettingsManager::new(
        f.repository.clone(),
        SettingsOptions {
            namespace: f.table.clone(),
            refresh_interval: Duration::from_secs(3600),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let weak = manager.downgrade();
    let scoped = settings(&manager, &f).await;
    let writer = f.site().await;
    writer.fill(json!({"name":"fresh now"})).await.unwrap();
    writer.save().await.unwrap();
    assert_eq!(scoped.get().await.name, "Default");
    scoped.refresh().await.unwrap();
    assert_eq!(scoped.get().await.name, "fresh now");
    manager.shutdown().await;
    drop(scoped);
    drop(manager);
    assert!(weak.upgrade().is_none());
    f.close().await;
}

#[tokio::test]
async fn sync_process_child() {
    let Some(input) = std::env::var_os("SETTINGS_SYNC_PROCESS") else {
        return;
    };
    let cfg: serde_json::Value = serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    let config = Repository::new(BTreeMap::from([(
        "database".into(),
        cfg["database"].clone(),
    )]));
    let database = DatabaseManager::new(
        Arc::new(Repository::new(config.all())),
        std::env::temp_dir(),
    );
    let connection = database.connection(None).unwrap();
    let source = Arc::new(
        openrust_settings::DatabaseSettingsRepository::new(connection.clone())
            .with_table(cfg["table"].as_str().unwrap())
            .unwrap(),
    );
    let manager = SettingsManager::new(
        source,
        options(cfg["table"].as_str().unwrap()),
        std::env::var_os("SETTINGS_TEST_REDIS_URL").map(|_| cache()),
    )
    .unwrap();
    manager.load("site").await.unwrap();
    std::fs::write(std::env::var_os("SETTINGS_SYNC_READY").unwrap(), b"ready").unwrap();
    until(async || {
        Settings::<Site>::load(Arc::new(manager.clone()), &config, None)
            .await
            .unwrap()
            .get()
            .await
            .name
            == "process update"
    })
    .await;
    std::fs::write(std::env::var_os("SETTINGS_SYNC_SEEN").unwrap(), b"seen").unwrap();
    manager.shutdown().await;
    connection.disconnect();
}
struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[tokio::test]
async fn independent_processes_and_restarted_node_converge_without_replaying_notifications() {
    let f = Fixture::new().await;
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("source.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"database":f.config.get("database"),"table":f.table})).unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let spawn = |name: &str| {
        let log = std::fs::File::create(root.path().join(format!("{name}.log"))).unwrap();
        Process(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "sync_process_child", "--nocapture"])
                .env("SETTINGS_SYNC_PROCESS", &file)
                .env(
                    "SETTINGS_SYNC_READY",
                    root.path().join(format!("{name}.ready")),
                )
                .env(
                    "SETTINGS_SYNC_SEEN",
                    root.path().join(format!("{name}.seen")),
                )
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        )
    };
    let mut first = spawn("first");
    let mut offline = spawn("offline");
    until(async || {
        root.path().join("first.ready").exists() && root.path().join("offline.ready").exists()
    })
    .await;
    offline.0.kill().unwrap();
    offline.0.wait().unwrap();
    let editor = f.site().await;
    editor.fill(json!({"name":"process update"})).await.unwrap();
    editor.save().await.unwrap();
    until(async || root.path().join("first.seen").exists()).await;
    let mut recovered = spawn("recovered");
    until(async || root.path().join("recovered.seen").exists()).await;
    assert!(first.0.wait().unwrap().success());
    assert!(recovered.0.wait().unwrap().success());
    f.close().await;
}

#[tokio::test]
async fn concurrent_notifications_run_each_handler_once_and_repair_stale_cache_envelopes() {
    use openrust::{
        cache::{Ttl, Value},
        contracts::cache::Repository as _,
    };
    let f = Fixture::new().await;
    let shared = cache();
    let manager = SettingsManager::new(
        f.repository.clone(),
        options(&f.table),
        Some(shared.clone()),
    )
    .unwrap();
    // 停用定时任务，精确验收并发通知，不依赖运行时调度顺序。
    manager.shutdown().await;
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    manager
        .on_change("site", move |_| {
            let c = c.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .await
        .unwrap();
    let old = manager.load("site").await.unwrap();
    let writer = f.site().await;
    writer.fill(json!({"name":"concurrent"})).await.unwrap();
    writer.save().await.unwrap();
    let latest = f.repository.load("site").await.unwrap();
    let namespace: String = f
        .table
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let key = format!("settings:{namespace}:snapshot:site");
    // 故意注入旧版本快照，数据库版本不匹配时必须重新读取并修复同一个缓存键。
    shared
        .put(
            &key,
            &Value::Json(serde_json::to_value(old).unwrap()),
            Some(Ttl::Seconds(60)),
        )
        .await
        .unwrap();
    let notification = SettingsNotification {
        namespace: f.table.clone(),
        group: "site".into(),
        version: latest.version().map(str::to_owned),
    };
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let manager = manager.clone();
        let notification = notification.clone();
        tasks.push(tokio::spawn(async move {
            manager.notify(&notification).await.unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(settings(&manager, &f).await.get().await.name, "concurrent");
    let Some(Value::Json(repaired)) = shared.get(&key).await.unwrap() else {
        panic!("缓存快照未修复");
    };
    let repaired: SettingsSnapshot = serde_json::from_value(repaired).unwrap();
    assert_eq!(repaired.version(), latest.version());
    // 新节点复用已验证版本的缓存，不再完整查询数据库。
    let source = Arc::new(Unavailable {
        source: f.repository.clone(),
        offline: AtomicBool::new(false),
        versions: AtomicUsize::new(0),
        loads: AtomicUsize::new(0),
    });
    let second = SettingsManager::new(source.clone(), options(&f.table), Some(shared)).unwrap();
    second.shutdown().await;
    assert_eq!(settings(&second, &f).await.get().await.name, "concurrent");
    assert_eq!(source.loads.load(Ordering::SeqCst), 0);
    assert_eq!(source.versions.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn shared_snapshots_keep_encrypted_properties_and_notifications_contain_only_metadata() {
    use openrust::{
        cache::Value,
        contracts::cache::Repository as _,
        encryption::{Cipher, Encrypter},
    };
    let f = Fixture::new().await;
    let shared = cache();
    let manager = Arc::new(
        SettingsManager::new(
            f.repository.clone(),
            options(&f.table),
            Some(shared.clone()),
        )
        .unwrap(),
    );
    let key = Arc::new(Encrypter::new([17; 32], Cipher::Aes256Cbc).unwrap());
    let secrets = Settings::<support::Secrets>::load(manager.clone(), &f.config, Some(key.clone()))
        .await
        .unwrap();
    secrets
        .fill(json!({"credentials":{"key":"cache-private-token"}}))
        .await
        .unwrap();
    secrets.save().await.unwrap();
    let namespace: String = f
        .table
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let Some(Value::Json(snapshot)) = shared
        .get(&format!("settings:{namespace}:snapshot:module_delivery"))
        .await
        .unwrap()
    else {
        panic!("缺少缓存快照");
    };
    assert!(!snapshot.to_string().contains("cache-private-token"));
    let Some(Value::Json(hint)) = shared
        .get(&format!("settings:{namespace}:changed:module_delivery"))
        .await
        .unwrap()
    else {
        panic!("缺少变更提示");
    };
    let fields = hint.as_object().unwrap();
    assert_eq!(fields.len(), 3);
    for field in ["namespace", "group", "version"] {
        assert!(fields.contains_key(field));
    }
    let second = Arc::new(
        SettingsManager::new(f.repository.clone(), options(&f.table), Some(shared)).unwrap(),
    );
    let loaded = Settings::<support::Secrets>::load(second.clone(), &f.config, Some(key))
        .await
        .unwrap();
    assert_eq!(loaded.get().await.credentials["key"], "cache-private-token");
    manager.shutdown().await;
    second.shutdown().await;
    f.close().await;
}
