use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::Arc,
    time::Duration,
};

use openrust::{
    cache::{CacheError, CacheManager, Repository, Ttl, Value},
    contracts::{
        cache::{LockProvider as _, Repository as _},
        container::Container as _,
        foundation::Application as _,
    },
    foundation::Application,
};

#[path = "../config/mod.rs"]
mod config;

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(label: &str, env: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("openrust-cache-{}-{label}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".env"), env).unwrap();
        Self(root)
    }

    async fn cache(&self, name: Option<&str>) -> Arc<Repository> {
        let mut app = Application::configure(self.0.clone())
            .withConfig(config::load)
            .create()
            .unwrap();
        app.boot().await.unwrap();
        app.container()
            .get::<CacheManager>("cache")
            .unwrap()
            .store(name)
            .unwrap()
    }

    fn artisan(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", &self.0)
            .env("APP_ENV", "local")
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) {
        let output = self.artisan(args);
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn exercise(first: &Repository, second: &Repository, db: bool) {
    assert!(first.flush().await.unwrap());
    let ttl = Some(Ttl::Seconds(1));
    assert!(
        first
            .put("binary", &Value::Bytes(vec![0, 255, 11]), ttl)
            .await
            .unwrap()
    );
    assert_eq!(
        second.get("binary").await.unwrap(),
        Some(Value::Bytes(vec![0, 255, 11]))
    );
    assert!(first.touch("binary", Ttl::Seconds(2)).await.unwrap());
    assert!(!first.add("binary", &Value::from(1), ttl).await.unwrap());

    let first_value = Value::from(1);
    let second_value = Value::from(2);
    let (left, right) = tokio::join!(
        first.add("race", &first_value, ttl),
        second.add("race", &second_value, ttl),
    );
    assert_ne!(left.unwrap(), right.unwrap());
    let (left, right) = tokio::join!(first.pull("race"), second.pull("race"));
    assert_ne!(left.unwrap().is_some(), right.unwrap().is_some());
    assert!(first.missing("race").await.unwrap());

    if db {
        assert!(matches!(
            first.increment("counter", 1).await,
            Err(CacheError::MissingCounter)
        ));
    }
    assert!(first.forever("counter", &Value::from(0)).await.unwrap());
    let increments = async |repo: &Repository| {
        for _ in 0..8 {
            repo.increment("counter", 1).await.unwrap();
        }
    };
    tokio::join!(increments(first), increments(second));
    assert_eq!(first.get("counter").await.unwrap(), Some(Value::from(16)));
    assert_eq!(second.decrement("counter", 2).await.unwrap(), 14);
    assert!(
        first
            .put("counter", &Value::from(9), Some(Ttl::Seconds(0)))
            .await
            .unwrap()
    );
    assert!(second.get("counter").await.unwrap().is_none());

    let mut first_lock = first
        .lock("resource", Some(Duration::from_secs(3)), None)
        .unwrap();
    let mut contender = second
        .lock("resource", Some(Duration::from_secs(3)), None)
        .unwrap();
    let (left, right) = tokio::join!(first_lock.get(), contender.get());
    let (held, waiting) = if left.unwrap() {
        assert!(!right.unwrap());
        (&mut first_lock, &mut contender)
    } else {
        assert!(right.unwrap());
        (&mut contender, &mut first_lock)
    };
    assert!(matches!(
        waiting.block(Duration::from_millis(15)).await,
        Err(CacheError::LockTimeout)
    ));
    assert!(!waiting.release().await.unwrap());
    assert!(!waiting.refresh(None).await.unwrap());
    let mut restored = second.restore_lock("resource", held.owner()).unwrap();
    assert!(
        restored
            .refresh(Some(Duration::from_millis(120)))
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(waiting.get().await.unwrap());
    assert!(!restored.release().await.unwrap());
    assert!(waiting.release().await.unwrap());

    let mut timed = first
        .lock("retry", Some(Duration::from_millis(50)), None)
        .unwrap();
    let mut retry = second
        .lock("retry", Some(Duration::from_secs(1)), None)
        .unwrap();
    assert!(timed.get().await.unwrap());
    retry.block(Duration::from_secs(1)).await.unwrap();
    assert!(!timed.release().await.unwrap());
    retry.force_release().await.unwrap();

    assert!(
        first
            .put("expire", &Value::from("gone"), ttl)
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(1050)).await;
    assert!(second.get("expire").await.unwrap().is_none());
    assert!(second.get("counter").await.unwrap().is_none());
}

#[tokio::test]
async fn array_cache_expires_and_is_application_scoped() {
    let root = Sandbox::new("array", "CACHE_STORE=array\n");
    let cache = root.cache(None).await;
    exercise(&cache, &cache, false).await;
    cache.forever("isolated", &Value::from(7)).await.unwrap();
    assert!(
        root.cache(None)
            .await
            .get("isolated")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn file_cache_is_shared_by_applications_and_artisan_processes() {
    let root = Sandbox::new("file", "CACHE_STORE=file\n");
    let first = root.cache(None).await;
    let second = root.cache(None).await;
    exercise(&first, &second, false).await;

    first
        .forever("../../not-a-path", &Value::from(8))
        .await
        .unwrap();
    root.success(&["cache:forget", "../../not-a-path", "file"]);
    assert!(second.get("../../not-a-path").await.unwrap().is_none());
    first.forever("data", &Value::from(8)).await.unwrap();
    let mut lock = first.lock("persists", None, None).unwrap();
    assert!(lock.get().await.unwrap());
    let entries = root.0.join("storage/framework/cache/data/entries");
    let unrelated = entries.join("unrelated");
    let directory = entries.join(format!("{}.record", "f".repeat(64)));
    fs::write(&unrelated, "leave this file alone").unwrap();
    fs::create_dir(&directory).unwrap();
    #[cfg(unix)]
    let symlink = {
        let symlink = entries.join(format!("{}.record", "e".repeat(64)));
        std::os::unix::fs::symlink(&unrelated, &symlink).unwrap();
        symlink
    };
    root.success(&["cache:clear", "file"]);
    assert!(second.get("data").await.unwrap().is_none());
    assert!(unrelated.is_file());
    assert!(directory.is_dir());
    #[cfg(unix)]
    assert!(symlink.is_symlink());
    assert!(lock.release().await.unwrap());
}

async fn verify_database(root: &Sandbox) {
    root.success(&["migrate"]);
    let first = root.cache(None).await;
    let second = root.cache(None).await;
    exercise(&first, &second, true).await;
    first
        .forever("cleanup", &Value::from("value"))
        .await
        .unwrap();
    root.success(&["cache:clear"]);
    assert!(second.missing("cleanup").await.unwrap());
    root.success(&["migrate:reset"]);
}

#[tokio::test]
async fn database_cache_uses_real_sqlite_transactions() {
    let root = Sandbox::new("sqlite", "CACHE_STORE=database\nDB_CONNECTION=sqlite\n");
    fs::create_dir(root.0.join("database")).unwrap();
    fs::File::create(root.0.join("database/database.sqlite")).unwrap();
    verify_database(&root).await;
}

#[tokio::test]
async fn database_cache_uses_real_mysql_transactions() {
    let Ok(url) = std::env::var("OPENRUST_TEST_CACHE_MYSQL_URL") else {
        return;
    };
    let root = Sandbox::new(
        "mysql",
        &format!("CACHE_STORE=database\nDB_CONNECTION=mysql\nDB_URL={url}\n"),
    );
    verify_database(&root).await;
}
