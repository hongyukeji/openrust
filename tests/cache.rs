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
    assert!(!first_lock.isLocked().await.unwrap());
    assert!(first_lock.isOwnedBy(None).await.unwrap());
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
    assert!(waiting.isLocked().await.unwrap());
    assert!(!waiting.isOwnedByCurrentProcess().await.unwrap());
    assert!(waiting.isOwnedBy(held.owner()).await.unwrap());
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
    assert!(!restored.isLocked().await.unwrap());
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

#[tokio::test]
async fn mysql_and_redis_failover_retries_errors_and_prefers_the_recovered_primary() {
    use openrust::{
        cache::events::CacheFailedOver,
        config::{ConfigError, Environment},
        database::DatabaseManager,
        events::Dispatcher,
    };
    use serde_json::json;
    use std::{collections::BTreeMap, path::Path};

    fn failover_config(
        base: &Path,
        env: &Environment,
    ) -> Result<BTreeMap<String, serde_json::Value>, ConfigError> {
        let mut config = config::load(base, env)?;
        config.get_mut("cache").unwrap()["stores"]["failover"]["stores"] =
            json!(["database", "redis", "array"]);
        Ok(config)
    }

    let (Ok(mysql), Ok(redis)) = (
        std::env::var("OPENRUST_TEST_MYSQL_URL"),
        std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL"),
    ) else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_CACHE_FAILOVER").is_none());
        return;
    };
    let root = Sandbox::new(
        "mysql-redis-failover",
        &format!(
            "CACHE_STORE=failover\nCACHE_PREFIX=cache-failover-\nDB_CONNECTION=mysql\nDB_URL={mysql}\nDB_CACHE_TABLE=failover_probe\nREDIS_URL={redis}\n"
        ),
    );
    let mut app = Application::configure(root.0.clone())
        .withConfig(failover_config)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().make::<CacheManager>().unwrap();
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    let events = dispatcher.fake(&[CacheFailedOver::NAME]);
    let failover = manager.store(None).unwrap();
    let backup = manager.store(Some("redis")).unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();

    let value = Value::from("redis fallback");
    assert!(
        failover
            .put("cached", &value, Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    assert_eq!(backup.get("cached").await.unwrap(), Some(value.clone()));
    assert_eq!(failover.get("cached").await.unwrap(), Some(value.clone()));
    assert_eq!(events.dispatched(CacheFailedOver::NAME).len(), 1);
    assert_eq!(
        events.dispatched(CacheFailedOver::NAME)[0]
            .downcast_ref::<CacheFailedOver>()
            .unwrap()
            .storeName,
        "database"
    );

    db.statement("CREATE TABLE failover_probe (`key` VARCHAR(255) PRIMARY KEY, `value` LONGTEXT NOT NULL, expiration BIGINT NOT NULL)", &[]).await.unwrap();
    assert_eq!(failover.get("cached").await.unwrap(), None);
    assert!(matches!(
        failover.increment("new-counter", 1).await,
        Err(CacheError::MissingCounter)
    ));
    assert_eq!(backup.get("new-counter").await.unwrap(), None);
    let primary = manager.store(Some("database")).unwrap();
    let recovered = Value::from("mysql recovered");
    assert!(
        failover
            .add("cached", &recovered, Some(Ttl::Seconds(20)))
            .await
            .unwrap()
    );
    assert!(primary.forever("primary-only", &recovered).await.unwrap());
    assert!(
        !failover
            .add("primary-only", &Value::from("ignored"), None)
            .await
            .unwrap()
    );
    assert_eq!(backup.get("primary-only").await.unwrap(), None);
    assert_eq!(primary.get("cached").await.unwrap(), Some(recovered));
    assert_eq!(backup.get("cached").await.unwrap(), Some(value.clone()));
    assert_eq!(events.dispatched(CacheFailedOver::NAME).len(), 1);

    db.statement("DROP TABLE failover_probe", &[])
        .await
        .unwrap();
    assert_eq!(failover.pull("cached").await.unwrap(), Some(value));
    assert_eq!(backup.get("cached").await.unwrap(), None);
    assert_eq!(events.dispatched(CacheFailedOver::NAME).len(), 2);

    // A failover repository can still scope tagged values while its first store is unavailable.
    assert!(failover.supportsTags());
    let tagged = failover.tags(&["batch", "reports"]).unwrap();
    assert!(tagged.forever("status", &"backup".into()).await.unwrap());
    assert_eq!(tagged.get("status").await.unwrap(), Some("backup".into()));
    assert!(failover.tags(&["reports"]).unwrap().flush().await.unwrap());
    assert_eq!(tagged.get("status").await.unwrap(), None);

    root.success(&["cache:clear", "failover"]);
    root.success(&["inspire"]);
}

#[tokio::test]
async fn failover_resolves_named_stores_lazily_and_keeps_the_last_error() {
    use openrust::config::Repository as Config;
    use openrust::database::DatabaseManager;
    use serde_json::json;

    let config = Arc::new(Config::new(
        [(
            "cache".into(),
            json!({
                "default":"failover",
                "stores":{
                    "failover":{"driver":"failover","stores":["unconfigured","array"]},
                    "array":{"driver":"array"},
                    "secondary-array":{"driver":"array"},
                    "lock-failover":{"driver":"failover","stores":["array","secondary-array"]},
                    "isolated":{"driver":"failover","stores":["missing-one","missing-two"]},
                    "cycle-a":{"driver":"failover","stores":["cycle-b"]},
                    "cycle-b":{"driver":"failover","stores":["cycle-a"]},
                    "empty":{"driver":"failover","stores":[]},
                }
            }),
        )]
        .into(),
    ));
    let manager = CacheManager::new(
        config.clone(),
        Arc::new(DatabaseManager::new(config, PathBuf::new())),
        PathBuf::new(),
    );
    let failover = manager.store(None).unwrap();
    assert!(
        failover
            .forever("value", &Value::from("saved"))
            .await
            .unwrap()
    );
    assert_eq!(
        failover.get("value").await.unwrap(),
        Some(Value::from("saved"))
    );
    let mut lock = failover
        .lock("lease", Some(Duration::from_secs(10)), Some("owner"))
        .unwrap();
    assert!(lock.get().await.unwrap());
    let mut restored = failover.restore_lock("lease", "owner").unwrap();
    assert!(restored.release().await.unwrap());
    let isolated = manager.store(Some("isolated")).unwrap();
    assert!(
        matches!(isolated.get("value").await, Err(CacheError::UnknownStore { name }) if name == "missing-two")
    );
    assert!(matches!(
        manager.store(Some("cycle-a")).unwrap().get("value").await,
        Err(CacheError::FailoverCycle)
    ));
    assert!(matches!(
        manager.store(Some("empty")),
        Err(CacheError::InvalidConfiguration)
    ));

    let first = manager.store(Some("array")).unwrap();
    let second = manager.store(Some("secondary-array")).unwrap();
    let mut first_lock = first.lock("batch", None, None).unwrap();
    let mut second_lock = second.lock("batch", None, None).unwrap();
    assert!(first_lock.get().await.unwrap());
    assert!(second_lock.get().await.unwrap());
    assert!(
        manager
            .store(Some("lock-failover"))
            .unwrap()
            .flushLocks()
            .await
            .unwrap()
    );
    assert!(first_lock.get().await.unwrap());
    assert!(second_lock.get().await.unwrap());
    assert!(matches!(
        manager.store(Some("cycle-a")).unwrap().flushLocks().await,
        Err(CacheError::FailoverCycle)
    ));
}

#[tokio::test]
async fn tagged_cache_preserves_order_duplicates_and_application_boundaries() {
    let root = Sandbox::new("tags", "CACHE_STORE=array\n");
    let cache = root.cache(None).await;
    assert!(cache.supportsTags());

    let ordered = cache.tags(&["people", "artists"]).unwrap();
    let reversed = cache.tags(&["artists", "people"]).unwrap();
    let duplicated = cache.tags(&["people", "artists", "artists"]).unwrap();
    let authors = cache.tags(&["authors"]).unwrap();
    let colon = cache.tags(&["people:artists"]).unwrap();
    let key = "profile:42";

    assert_eq!(
        duplicated.getTags().getNames(),
        ["people", "artists", "artists"]
    );
    let cases = [
        (&ordered, 1),
        (&reversed, 2),
        (&duplicated, 3),
        (&authors, 4),
        (&colon, 5),
    ];
    for (tagged, value) in cases {
        assert!(tagged.forever(key, &value.into()).await.unwrap());
    }
    for (tagged, value) in cases {
        assert_eq!(tagged.get(key).await.unwrap(), Some(value.into()));
    }
    cache.forever(key, &0.into()).await.unwrap();
    let metadata = ordered.getTags().getNamespace().await.unwrap();
    assert!(cache.tags(&["artists"]).unwrap().flush().await.unwrap());
    assert_ne!(ordered.getTags().getNamespace().await.unwrap(), metadata);
    for tagged in [&ordered, &reversed, &duplicated] {
        assert_eq!(tagged.get(key).await.unwrap(), None);
    }
    assert_eq!(authors.get(key).await.unwrap(), Some(4.into()));
    assert_eq!(colon.get(key).await.unwrap(), Some(5.into()));
    assert_eq!(cache.get(key).await.unwrap(), Some(0.into()));
    assert_eq!(
        root.cache(None)
            .await
            .tags(&["authors"])
            .unwrap()
            .get(key)
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        root.cache(Some("file")).await.tags(&["artists"]),
        Err(CacheError::TagsUnsupported)
    ));

    assert_eq!(ordered.increment(key, 2).await.unwrap(), 2);
    assert!(!ordered.add(key, &1.into(), None).await.unwrap());
    assert!(ordered.touch(key, Ttl::Seconds(60)).await.unwrap());
    assert_eq!(ordered.pull(key).await.unwrap(), Some(2.into()));
    assert_eq!(ordered.get(key).await.unwrap(), None);
    assert!(cache.flush().await.unwrap());
    assert_eq!(authors.get(key).await.unwrap(), None);
}

#[tokio::test]
async fn cache_tags_facade_writes_reads_and_flushes_over_http() {
    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{configuration::Routing, http::Kernel},
        http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
        routing::{Router, RoutingError},
        support::facades::Cache,
    };
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
        time::timeout,
    };

    async fn store_report() -> Result<bool, CacheError> {
        Cache::tags(&["reports", "daily"])?
            .put("latest", &Value::from("ready"), Some(Ttl::Seconds(60)))
            .await
    }

    fn endpoint(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        async fn action(path: &str) -> Result<serde_json::Value, CacheError> {
            match path {
                "/write" => Ok(serde_json::json!(store_report().await?)),
                "/flush" => {
                    Cache::tags(&["daily"])?.flush().await?;
                    Ok(serde_json::Value::Null)
                }
                _ => {
                    let value = Cache::tags(&["reports", "daily"])?.get("latest").await?;
                    Ok(serde_json::json!(value.as_ref().and_then(Value::as_str)))
                }
            }
        }

        Box::pin(async move {
            let value =
                action(request.uri().path())
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
            Response::json(StatusCode::OK, value)
        })
    }

    fn routes(_: &Application) -> Result<Router, RoutingError> {
        let mut router = Router::new();
        router.r#match(&[Method::POST], "/write", endpoint)?;
        router.get("/read", endpoint)?;
        router.r#match(&[Method::POST], "/flush", endpoint)?;
        Ok(router)
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handleTransport(incoming).await
    }

    let env = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL").map_or_else(
        |_| "CACHE_STORE=array\n".to_owned(),
        |url| format!("CACHE_STORE=redis\nCACHE_PREFIX=tags-http-\nREDIS_URL={url}\n"),
    );
    for (env, persists) in [(env.as_str(), true), ("CACHE_STORE=null\n", false)] {
        let root = Sandbox::new("tags-http", env);
        let mut kernel = Kernel::new(
            Application::configure(root.0.clone())
                .withConfig(config::load)
                .withRouting(Routing::new(routes))
                .create()
                .unwrap(),
        );
        kernel.bootstrap().await.unwrap();
        let dispatcher = kernel
            .get_application()
            .container()
            .make::<openrust::events::Dispatcher>()
            .unwrap();
        let events = dispatcher.fake(&[
            openrust::cache::events::KeyWritten::NAME,
            openrust::cache::events::KeyWriteFailed::NAME,
            openrust::cache::events::CacheFlushed::NAME,
        ]);
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

        timeout(Duration::from_secs(5), async {
            for (method, path, expected) in [
                ("GET", "/read", "null"),
                ("POST", "/write", if persists { "true" } else { "false" }),
                ("GET", "/read", if persists { "\"ready\"" } else { "null" }),
                ("POST", "/flush", "null"),
                ("GET", "/read", "null"),
            ] {
                let mut stream = TcpStream::connect(address).await.unwrap();
                stream
                .write_all(
                    format!(
                        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).await.unwrap();
                assert!(response.starts_with("HTTP/1.1 200 "), "{path}: {response}");
                assert_eq!(response.split_once("\r\n\r\n").unwrap().1, expected);
            }
        })
        .await
        .unwrap();
        server.abort();
        let _ = server.await;
        events.assertDispatchedTimes(
            openrust::cache::events::KeyWritten::NAME,
            usize::from(persists),
        );
        events.assertDispatchedTimes(
            openrust::cache::events::KeyWriteFailed::NAME,
            usize::from(!persists),
        );
        events.assertDispatchedTimes(openrust::cache::events::CacheFlushed::NAME, 1);
        root.success(&["inspire"]);
    }
}

#[tokio::test]
async fn redis_tags_physically_flush_values_and_prune_stale_indexes() {
    use openrust::redis::{RedisManager, Value as RedisValue};

    let Ok(redis) = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_CACHE_FAILOVER").is_none());
        return;
    };
    let root = Sandbox::new(
        "redis-tags",
        &format!(
            "CACHE_STORE=redis\nCACHE_PREFIX=cache*?-\nREDIS_PREFIX=tags-\nREDIS_URL={redis}\n"
        ),
    );
    let mut app = Application::configure(root.0.clone())
        .withConfig(config::load)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().make::<CacheManager>().unwrap();
    let plain = manager.store(None).unwrap();
    let redis = app
        .container()
        .make::<RedisManager>()
        .unwrap()
        .connection(Some("cache"))
        .unwrap();
    let exists = async |key: &str| {
        redis.command("EXISTS", &[key.as_bytes()]).await.unwrap() == RedisValue::Int(1)
    };
    let cardinality = async |key: &str| redis.command("ZCARD", &[key.as_bytes()]).await.unwrap();
    let index = async |name: &str| {
        let tags = plain.tags(&[name]).unwrap();
        let id = tags.getTags().tagId(name).await.unwrap();
        format!("tags-cache*?-tagged:v1:index:{id}")
    };

    assert!(plain.supportsTags());
    assert!(plain.forever("same", &0.into()).await.unwrap());
    let ordered = plain.tags(&["people", "artists"]).unwrap();
    let reversed = plain.tags(&["artists", "people"]).unwrap();
    let repeated = plain.tags(&["artists", "people", "people"]).unwrap();
    let unrelated = plain.tags(&["authors"]).unwrap();
    for (tags, value) in [
        (&ordered, 1),
        (&reversed, 2),
        (&repeated, 3),
        (&unrelated, 4),
    ] {
        assert!(tags.forever("same", &value.into()).await.unwrap());
    }
    for (tags, value) in [
        (&ordered, 1),
        (&reversed, 2),
        (&repeated, 3),
        (&unrelated, 4),
    ] {
        assert_eq!(tags.get("same").await.unwrap(), Some(value.into()));
    }
    let artist_index = index("artists").await;
    let people_index = index("people").await;
    let mut physical = vec![
        ordered.taggedItemKey("same").await.unwrap(),
        reversed.taggedItemKey("same").await.unwrap(),
        repeated.taggedItemKey("same").await.unwrap(),
    ];
    for number in 0..130 {
        let key = number.to_string();
        assert!(ordered.add(&key, &number.into(), None).await.unwrap());
        physical.push(ordered.taggedItemKey(&key).await.unwrap());
    }
    assert!(!ordered.add("same", &19.into(), None).await.unwrap());
    assert_eq!(cardinality(&artist_index).await, RedisValue::Int(133));
    for key in &physical {
        assert!(exists(key).await);
    }
    assert!(plain.tags(&["artists"]).unwrap().flush().await.unwrap());
    for key in &physical {
        assert!(!exists(key).await);
    }
    assert_eq!(cardinality(&artist_index).await, RedisValue::Int(0));
    assert_eq!(unrelated.get("same").await.unwrap(), Some(4.into()));
    assert_eq!(plain.get("same").await.unwrap(), Some(0.into()));

    let authors = index("authors").await;
    assert!(
        unrelated
            .put("short", &1.into(), Some(Ttl::Seconds(2)))
            .await
            .unwrap()
    );
    assert!(unrelated.touch("short", Ttl::Seconds(1)).await.unwrap());
    assert_eq!(
        unrelated.increment("counter", i64::MAX).await.unwrap(),
        i64::MAX
    );
    assert_eq!(
        unrelated.decrement("counter", 2).await.unwrap(),
        i64::MAX - 2
    );
    assert_eq!(
        unrelated.pull("counter").await.unwrap(),
        Some((i64::MAX - 2).into())
    );
    assert_eq!(cardinality(&authors).await, RedisValue::Int(2));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(unrelated.get("short").await.unwrap(), None);
    root.success(&["cache:prune-stale-tags", "redis"]);
    assert_eq!(cardinality(&authors).await, RedisValue::Int(1));
    assert_eq!(cardinality(&people_index).await, RedisValue::Int(0));
    assert_eq!(unrelated.get("same").await.unwrap(), Some(4.into()));

    let old_key = unrelated.taggedItemKey("same").await.unwrap();
    // An invalid reference interrupts the first drain without deleting ordinary keys.
    let ordinary = "tags-cache*?-same";
    redis
        .command("ZADD", &[authors.as_bytes(), b"-2", ordinary.as_bytes()])
        .await
        .unwrap();
    assert!(unrelated.flush().await.is_err());
    assert!(exists(ordinary).await);
    assert!(exists(&old_key).await);
    assert_eq!(unrelated.get("same").await.unwrap(), None);
    assert!(unrelated.forever("same", &5.into()).await.unwrap());
    assert_ne!(unrelated.taggedItemKey("same").await.unwrap(), old_key);
    assert_eq!(unrelated.get("same").await.unwrap(), Some(5.into()));
    redis
        .command("ZREM", &[authors.as_bytes(), ordinary.as_bytes()])
        .await
        .unwrap();
    assert!(unrelated.flush().await.unwrap());
    assert!(!exists(&old_key).await);
    assert_eq!(unrelated.get("same").await.unwrap(), None);
    assert_eq!(plain.get("same").await.unwrap(), Some(0.into()));

    let before_clear = plain
        .tags(&["new"])
        .unwrap()
        .taggedItemKey("value")
        .await
        .unwrap();
    assert!(plain.flush().await.unwrap());
    assert_ne!(
        plain
            .tags(&["new"])
            .unwrap()
            .taggedItemKey("value")
            .await
            .unwrap(),
        before_clear
    );
}

#[tokio::test]
async fn clearing_array_and_file_locks_preserves_data_and_gate() {
    for store in ["array", "file"] {
        let root = Sandbox::new("flush-locks", &format!("CACHE_STORE={store}\n"));
        let cache = root.cache(None).await;
        let lock = || cache.lock("shared", None, None).unwrap();
        assert!(cache.supportsFlushingLocks());
        assert!(cache.forever("shared", &"data".into()).await.unwrap());
        let mut first = lock();
        let mut second = lock();
        assert!(first.get().await.unwrap());
        assert!(!second.get().await.unwrap());
        if store == "file" {
            let locks = root.0.join("storage/framework/cache/data/locks");
            fs::write(locks.join("other"), "keep").unwrap();
            root.success(&["cache:clear", "file", "--locks"]);
            assert!(locks.join(".gate").is_file());
            assert_eq!(fs::read_to_string(locks.join("other")).unwrap(), "keep");
        } else {
            assert!(cache.flushLocks().await.unwrap());
        }
        assert!(second.get().await.unwrap());
        assert!(!first.release().await.unwrap());
        assert!(second.release().await.unwrap());
        assert_eq!(cache.get("shared").await.unwrap(), Some("data".into()));
    }
}

#[tokio::test]
async fn artisan_clears_mysql_lock_table_and_rejects_shared_table() {
    let Ok(url) = std::env::var("OPENRUST_TEST_CACHE_MYSQL_URL") else {
        return;
    };
    let root = Sandbox::new(
        "mysql-flush-locks",
        &format!("CACHE_STORE=database\nDB_CONNECTION=mysql\nDB_URL={url}\n"),
    );
    root.success(&["migrate"]);
    let cache = root.cache(None).await;
    cache.forever("shared", &"database".into()).await.unwrap();
    let mut first = cache.lock("shared", None, None).unwrap();
    let mut second = cache.lock("shared", None, None).unwrap();
    assert!(first.get().await.unwrap());
    assert!(!second.get().await.unwrap());
    let conflict = root.artisan(&["cache:clear", "database", "--locks", "--tags=reports"]);
    assert_eq!(conflict.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("Cache tags cannot"));
    assert!(!second.get().await.unwrap());
    root.success(&["cache:clear", "database", "--locks"]);
    assert!(second.get().await.unwrap());
    assert_eq!(cache.get("shared").await.unwrap(), Some("database".into()));

    let shared = Sandbox::new(
        "mysql-shared-locks",
        &format!(
            "CACHE_STORE=database\nDB_CONNECTION=mysql\nDB_URL={url}\nDB_CACHE_LOCK_TABLE=cache\n"
        ),
    );
    let result = shared.artisan(&["cache:clear", "--locks"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires a separate lock store"));
    assert_eq!(cache.get("shared").await.unwrap(), Some("database".into()));
    root.success(&["migrate:reset"]);
}

#[tokio::test]
async fn artisan_redis_lock_flush_uses_selected_database_and_preserves_tagged_values() {
    let Ok(url) = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_CACHE_FAILOVER").is_none());
        return;
    };
    let settings = format!(
        "CACHE_STORE=redis\nREDIS_URL={url}\nCACHE_PREFIX=redis-clear-locks-\nREDIS_CACHE_DB=2\n"
    );
    let root = Sandbox::new("redis-flush-locks", &format!("{settings}REDIS_DB=3\n"));
    let same = Sandbox::new("redis-shared-locks", &format!("{settings}REDIS_DB=2\n"));
    let cache = root.cache(None).await;
    let tagged = cache.tags(&["keep"]).unwrap();
    assert!(cache.forever("shared", &"redis".into()).await.unwrap());
    assert!(tagged.forever("shared", &"tagged".into()).await.unwrap());

    let shared = same.cache(None).await;
    let mut shared_lock = shared.lock("data-db-lock", None, None).unwrap();
    let mut shared_probe = shared.lock("data-db-lock", None, None).unwrap();
    assert!(shared_lock.get().await.unwrap());
    assert!(matches!(
        shared.flushLocks().await,
        Err(CacheError::LockStoreNotSeparate)
    ));
    let result = same.artisan(&["cache:clear", "redis", "--locks"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!shared_probe.get().await.unwrap());

    let mut first = cache.lock("shared", None, None).unwrap();
    let mut second = cache.lock("shared", None, None).unwrap();
    assert!(first.get().await.unwrap());
    assert!(!second.get().await.unwrap());
    root.success(&["cache:clear", "redis", "--locks"]);
    assert!(second.get().await.unwrap());
    assert!(!first.release().await.unwrap());
    assert!(!shared_probe.get().await.unwrap());
    assert_eq!(cache.get("shared").await.unwrap(), Some("redis".into()));
    assert_eq!(tagged.get("shared").await.unwrap(), Some("tagged".into()));

    root.success(&["cache:clear", "redis", "--tags=keep"]);
    assert_eq!(cache.get("shared").await.unwrap(), Some("redis".into()));
    assert_eq!(tagged.get("shared").await.unwrap(), None);
    assert!(second.release().await.unwrap());
    assert!(shared_lock.release().await.unwrap());
}

#[tokio::test]
async fn redis_lock_flush_distinguishes_servers_from_aliases() {
    use openrust::redis::RedisManager;

    fn configure(
        base: &std::path::Path,
        env: &openrust::config::Environment,
    ) -> Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError>
    {
        let mut values = config::load(base, env)?;
        let second = std::fs::read_to_string(base.join("redis-lock-url")).unwrap();
        values.get_mut("database").unwrap()["redis"]["default"]["url"] = second.into();
        Ok(values)
    }

    let (Ok(url), Ok(lock_url)) = (
        std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL"),
        std::env::var("OPENRUST_TEST_LOCK_REDIS_URL"),
    ) else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS_LOCK_SERVERS").is_none());
        return;
    };
    let env = format!("CACHE_STORE=redis\nREDIS_URL={url}\nREDIS_DB=4\n");
    let separate = Sandbox::new("redis-two-lock-servers", &env);
    let shared = Sandbox::new("redis-lock-alias", &env);
    fs::write(separate.0.join("redis-lock-url"), lock_url).unwrap();
    fs::write(
        shared.0.join("redis-lock-url"),
        url.replace("127.0.0.1", "localhost"),
    )
    .unwrap();

    let mut separate_app = Application::configure(separate.0.clone())
        .withConfig(configure)
        .create()
        .unwrap();
    let mut shared_app = Application::configure(shared.0.clone())
        .withConfig(configure)
        .create()
        .unwrap();
    separate_app.boot().await.unwrap();
    shared_app.boot().await.unwrap();
    let cache = separate_app
        .container()
        .make::<CacheManager>()
        .unwrap()
        .store(None)
        .unwrap();
    let same = shared_app
        .container()
        .make::<CacheManager>()
        .unwrap()
        .store(None)
        .unwrap();
    let manager = separate_app.container().make::<RedisManager>().unwrap();
    let backend = manager.connection(Some("default")).unwrap();
    backend.set("locks-db-marker", b"dedicated").await.unwrap();

    assert!(cache.forever("shared", &"data".into()).await.unwrap());
    let mut alias = same.lock("alias", None, None).unwrap();
    let mut probe = same.lock("alias", None, None).unwrap();
    assert!(alias.get().await.unwrap());
    assert!(matches!(
        same.flushLocks().await,
        Err(CacheError::LockStoreNotSeparate)
    ));
    assert!(!probe.get().await.unwrap());

    let mut original = cache.lock("alias", None, None).unwrap();
    let mut replacement = cache.lock("alias", None, None).unwrap();
    assert!(original.get().await.unwrap());
    assert!(cache.flushLocks().await.unwrap());
    assert!(replacement.get().await.unwrap());
    assert!(!probe.get().await.unwrap());
    assert_eq!(backend.get("locks-db-marker").await.unwrap(), None);
    assert_eq!(cache.get("shared").await.unwrap(), Some("data".into()));
}

#[tokio::test]
async fn null_store_discards_values_and_returns_nonexclusive_lock_handles() {
    let mut fixture = openrust::testing::TestApp::create(|builder| builder).unwrap();
    fixture.boot().await.unwrap();
    let manager = fixture.app().container().make::<CacheManager>().unwrap();
    assert_eq!(manager.getDefaultDriver().unwrap(), "null");
    let unconfigured = manager.store(None).unwrap();
    assert!(Arc::ptr_eq(
        &unconfigured,
        &manager.store(Some("null")).unwrap()
    ));
    assert_eq!(unconfigured.get("unconfigured").await.unwrap(), None);

    let root = Sandbox::new("null-cache", "CACHE_STORE=null\n");
    let cache = root.cache(None).await;
    assert!(cache.supportsTags());
    assert!(cache.supportsFlushingLocks());
    assert!(
        !cache
            .get_store()
            .lock_flusher()
            .unwrap()
            .hasSeparateLockStore()
    );
    assert_eq!(cache.get_store().get_prefix(), "");
    assert_eq!(cache.get("one").await.unwrap(), None);
    assert_eq!(
        cache.many(&["one", "two", "one"]).await.unwrap(),
        [None, None, None]
    );
    assert!(
        !cache
            .put("one", &1.into(), Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    assert!(
        !cache
            .putMany(&[("one".into(), 1.into())], Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    assert!(!cache.forever("one", &1.into()).await.unwrap());
    assert!(!cache.add("one", &1.into(), None).await.unwrap());
    assert!(!cache.touch("one", Ttl::Seconds(30)).await.unwrap());
    assert_eq!(cache.pull("one").await.unwrap(), None);
    assert!(matches!(
        cache.increment("one", 1).await,
        Err(CacheError::MissingCounter)
    ));
    assert!(matches!(
        cache.decrement("one", 1).await,
        Err(CacheError::MissingCounter)
    ));
    assert!(cache.forget("one").await.unwrap());
    let tagged = cache.tags(&["reports", "reports"]).unwrap();
    assert!(!tagged.forever("one", &1.into()).await.unwrap());
    assert_eq!(tagged.get("one").await.unwrap(), None);
    assert!(tagged.flush().await.unwrap());

    let first = cache.lock("no-lock", None, None).unwrap();
    let mut second = cache.lock("no-lock", None, None).unwrap();
    let mut first = first.betweenBlockedAttemptsSleepFor(0).unwrap();
    assert!(!first.isLocked().await.unwrap());
    assert!(first.isOwnedByCurrentProcess().await.unwrap());
    assert!(!first.isOwnedBy(None).await.unwrap());
    assert_ne!(first.owner(), second.owner());
    assert!(first.get().await.unwrap());
    assert!(second.get().await.unwrap());
    assert!(second.refresh(None).await.unwrap());
    assert!(second.release().await.unwrap());
    let mut restored = cache.restore_lock("no-lock", first.owner()).unwrap();
    assert_eq!(restored.owner(), first.owner());
    restored.block(Duration::ZERO).await.unwrap();
    restored.force_release().await.unwrap();
    root.success(&["cache:forget", "reports/latest", "null"]);
    root.success(&["cache:clear", "null", "--tags=reports"]);
    root.success(&["cache:clear", "null", "--locks"]);
    root.success(&["cache:clear", "null"]);
}

#[tokio::test]
async fn null_results_end_failover_and_emit_laravel_event_payloads() {
    use openrust::{
        cache::events::{CacheFailedOver, CacheMissed, KeyWriteFailed},
        events::Dispatcher,
    };

    let root = Sandbox::new("null-failover-cache", "CACHE_STORE=failover\n");
    let mut app = Application::configure(root.0.clone())
        .withConfig(|base, env| {
            let mut values = config::load(base, env)?;
            values.get_mut("cache").unwrap()["stores"]["failover"]["stores"] =
                serde_json::json!(["null", "array"]);
            Ok(values)
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    let events = dispatcher.fake(&[
        CacheFailedOver::NAME,
        CacheMissed::NAME,
        KeyWriteFailed::NAME,
    ]);
    let manager = app.container().make::<CacheManager>().unwrap();
    let array = manager.store(Some("array")).unwrap();
    let failover = manager.store(None).unwrap();
    assert!(
        !failover
            .put("one", &1.into(), Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    assert!(array.forever("two", &2.into()).await.unwrap());
    assert_eq!(failover.get("two").await.unwrap(), None);
    assert_eq!(array.get("one").await.unwrap(), None);
    assert!(matches!(
        failover.increment("two", 1).await,
        Err(CacheError::MissingCounter)
    ));
    assert_eq!(array.get("two").await.unwrap(), Some(2.into()));
    events.assertNotDispatched(CacheFailedOver::NAME);
    let misses = events.dispatched(CacheMissed::NAME);
    let miss = misses[0].downcast_ref::<CacheMissed>().unwrap();
    assert_eq!(miss.storeName, None);
    assert_eq!(miss.key, "two");
    events.assertDispatchedTimes(KeyWriteFailed::NAME, 1);
}

#[tokio::test]
async fn storage_cache_reuses_disk_and_preserves_files_outside_its_directory() {
    use openrust::filesystem::FilesystemManager;

    let root = Sandbox::new(
        "storage-cache",
        "CACHE_STORE=storage\nCACHE_STORAGE_DISK=local\nCACHE_STORAGE_PATH=/framework/cache/items/\nCACHE_PREFIX=fixture-\n",
    );
    let mut app = Application::configure(root.0.clone())
        .withConfig(config::load)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().make::<CacheManager>().unwrap();
    let disk = app
        .container()
        .make::<FilesystemManager>()
        .unwrap()
        .disk(None)
        .unwrap();
    let cache = manager.store(None).unwrap();
    assert!(!cache.supportsTags());
    assert!(!cache.supportsFlushingLocks());
    assert!(matches!(
        cache.lock("outside", None, None),
        Err(CacheError::LocksUnsupported)
    ));
    assert_eq!(cache.get_store().get_prefix(), "fixture-");

    assert!(
        disk.put("framework/cache/items-other/keep.txt", b"keep")
            .await
            .unwrap()
    );
    assert!(disk.put("ordinary.txt", b"keep").await.unwrap());
    let key = "../../a/b";
    let content = Value::Bytes(vec![0, 255, 1]);
    assert!(
        cache
            .put(key, &content, Some(Ttl::Seconds(2)))
            .await
            .unwrap()
    );
    let hashed = disk.allFiles(Some("framework/cache/items")).await.unwrap();
    assert_eq!(hashed.len(), 1);
    assert_eq!(
        hashed[0],
        "framework/cache/items/6a/d5/6ad532da2efe59a63b70cd7ed5bfac2233576b4d"
    );
    assert!(!cache.add(key, &1.into(), None).await.unwrap());
    assert!(cache.touch(key, Ttl::Seconds(1)).await.unwrap());
    assert_eq!(
        cache.many(&[key, "missing"]).await.unwrap(),
        [Some(content), None]
    );

    assert_eq!(cache.increment("counter", 9).await.unwrap(), 9);
    assert_eq!(cache.decrement("counter", 5).await.unwrap(), 4);
    assert_eq!(cache.pull("counter").await.unwrap(), Some(4.into()));
    assert_eq!(cache.get("counter").await.unwrap(), None);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(cache.get(key).await.unwrap(), None);
    assert!(disk.fileExists(&hashed[0]).await.unwrap());
    assert!(disk.put(&hashed[0], b"invalid cache record").await.unwrap());
    assert_eq!(cache.get(key).await.unwrap(), None);
    assert!(cache.forever(key, &"fresh".into()).await.unwrap());
    assert_eq!(
        root.cache(None).await.get(key).await.unwrap(),
        Some("fresh".into())
    );
    root.success(&["cache:forget", key, "storage"]);
    assert_eq!(cache.get(key).await.unwrap(), None);
    assert!(cache.forever(key, &"fresh".into()).await.unwrap());
    root.success(&["cache:clear", "storage"]);
    assert!(disk.directoryExists("framework/cache/items").await.unwrap());
    assert!(
        disk.allFiles(Some("framework/cache/items"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        disk.get("framework/cache/items-other/keep.txt")
            .await
            .unwrap()
            .unwrap(),
        b"keep"
    );
    assert_eq!(disk.get("ordinary.txt").await.unwrap().unwrap(), b"keep");

    let public = app
        .container()
        .make::<FilesystemManager>()
        .unwrap()
        .disk(Some("public"))
        .unwrap();
    assert!(public.put("keep.txt", b"separate disk").await.unwrap());
    let entire_disk = manager
        .build(serde_json::json!({"driver": "storage", "path": ""}))
        .unwrap();
    assert!(entire_disk.flush().await.unwrap());
    assert!(disk.allFiles(None).await.unwrap().is_empty());
    assert_eq!(
        public.get("keep.txt").await.unwrap().unwrap(),
        b"separate disk"
    );
}

#[tokio::test]
async fn storage_cache_s3_flush_keeps_adjacent_prefixes() {
    use openrust::filesystem::FilesystemManager;

    let (Ok(endpoint), Ok(bucket)) = (
        std::env::var("OPENRUST_TEST_CACHE_S3_ENDPOINT"),
        std::env::var("OPENRUST_TEST_CACHE_S3_BUCKET"),
    ) else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_CACHE_S3").is_none());
        return;
    };
    let root = Sandbox::new(
        "s3-storage-cache",
        &format!(
            "CACHE_STORE=storage\nCACHE_STORAGE_DISK=s3\nCACHE_STORAGE_PATH=tenant/cache\nAWS_ENDPOINT={endpoint}\nAWS_BUCKET={bucket}\nAWS_ACCESS_KEY_ID=cache-fixture\nAWS_SECRET_ACCESS_KEY=local-cache-fixture\nAWS_USE_PATH_STYLE_ENDPOINT=true\n"
        ),
    );
    let mut app = Application::configure(root.0.clone())
        .withConfig(config::load)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().make::<CacheManager>().unwrap();
    let disks = app.container().make::<FilesystemManager>().unwrap();
    let disk = disks.disk(Some("s3")).unwrap();
    let cache = manager.store(None).unwrap();
    if std::env::var_os("OPENRUST_TEST_CACHE_S3_PAGED").is_some() {
        let objects = disk.allFiles(Some("paginated")).await.unwrap();
        assert_eq!(objects.len(), 1002);
        assert_eq!(objects.first().unwrap(), "paginated/0000");
        assert_eq!(objects.last().unwrap(), "paginated/1001");
    }

    assert!(
        disk.put("tenant/cache-other/keep.txt", b"adjacent")
            .await
            .unwrap()
    );
    assert!(
        disk.put("tenant-b/keep.txt", b"different tenant")
            .await
            .unwrap()
    );
    assert!(cache.add("one", &"first".into(), None).await.unwrap());
    assert!(!cache.add("one", &"second".into(), None).await.unwrap());
    assert!(cache.forever("two", &vec![1, 0, 255].into()).await.unwrap());
    assert_eq!(cache.get("one").await.unwrap(), Some("first".into()));
    assert_eq!(
        cache.get("two").await.unwrap(),
        Some(vec![1, 0, 255].into())
    );
    assert_eq!(disk.allFiles(Some("tenant/cache")).await.unwrap().len(), 2);
    root.success(&["cache:clear", "storage"]);
    assert_eq!(cache.get("one").await.unwrap(), None);
    assert_eq!(cache.get("two").await.unwrap(), None);
    assert!(
        disk.allFiles(Some("tenant/cache"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        disk.get("tenant/cache-other/keep.txt")
            .await
            .unwrap()
            .unwrap(),
        b"adjacent"
    );
    assert_eq!(
        disk.get("tenant-b/keep.txt").await.unwrap().unwrap(),
        b"different tenant"
    );
}

#[tokio::test]
async fn memcached_values_tags_and_owner_checked_locks_are_shared() {
    let Ok(port) = std::env::var("MEMCACHED_FIXTURE_PORT") else {
        assert!(std::env::var_os("MEMCACHED_FIXTURE_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new(
        "memcached",
        &format!("CACHE_STORE=memcached\nMEMCACHED_PORT={port}\nCACHE_PREFIX=acceptance:\n"),
    );
    let first = root.cache(None).await;
    let second = root.cache(None).await;
    assert!(first.flush().await.unwrap());
    let binary = Value::Bytes(vec![0, 255, 11]);
    let ttl = Some(Ttl::Seconds(1));
    let values = vec![
        ("binary".into(), binary.clone()),
        ("number".into(), 7.into()),
        ("numeric-string".into(), "007".into()),
    ];
    assert!(first.putMany(&values, Some(Ttl::Seconds(2))).await.unwrap());
    assert_eq!(
        second
            .many(&["number", "missing", "binary", "number", "numeric-string"])
            .await
            .unwrap(),
        [
            Some(7.into()),
            None,
            Some(binary.clone()),
            Some(7.into()),
            Some("007".into())
        ]
    );
    assert!(
        second
            .touch("binary", Ttl::Seconds(35 * 24 * 60 * 60))
            .await
            .unwrap()
    );
    assert!(!first.touch("missing", Ttl::Seconds(30)).await.unwrap());
    assert!(first.put("expiring", &1.into(), ttl).await.unwrap());
    assert_eq!(second.increment("expiring", 2).await.unwrap(), 3);
    assert_eq!(second.decrement("number", 20).await.unwrap(), 0);
    assert!(matches!(
        first.increment("missing", 1).await,
        Err(CacheError::MissingCounter)
    ));
    assert!(matches!(
        first.increment("numeric-string", 1).await,
        Err(CacheError::InvalidValue)
    ));
    assert!(matches!(
        first.get("bad key\r\n").await,
        Err(CacheError::InvalidValue)
    ));
    assert!(matches!(
        first
            .forever("large", &Value::Bytes(vec![0; 1024 * 1024]))
            .await,
        Err(CacheError::InvalidValue)
    ));

    let value = Value::from(2);
    let (left, right) = tokio::join!(
        first.add("race", &value, ttl),
        second.add("race", &value, ttl)
    );
    assert_ne!(left.unwrap(), right.unwrap());
    let (left, right) = tokio::join!(first.pull("race"), second.pull("race"));
    assert_ne!(left.unwrap().is_some(), right.unwrap().is_some());

    let tagged = first.tags(&["authors", "reports"]).unwrap();
    assert!(tagged.forever("one", &11.into()).await.unwrap());
    assert_eq!(
        second
            .tags(&["authors", "reports"])
            .unwrap()
            .get("one")
            .await
            .unwrap(),
        Some(11.into())
    );
    first.tags(&["authors"]).unwrap().flush().await.unwrap();
    assert!(tagged.get("one").await.unwrap().is_none());
    assert_eq!(first.get("binary").await.unwrap(), Some(binary));

    let mut owner = first
        .lock("rebuild", Some(Duration::from_secs(1)), Some("first"))
        .unwrap();
    let mut other = second
        .lock("rebuild", Some(Duration::from_secs(3)), Some("second"))
        .unwrap();
    assert!(!owner.isLocked().await.unwrap());
    assert!(owner.get().await.unwrap());
    assert!(owner.isOwnedByCurrentProcess().await.unwrap());
    assert!(other.isOwnedBy("first").await.unwrap());
    assert!(!other.isOwnedByCurrentProcess().await.unwrap());
    assert!(!other.get().await.unwrap());
    assert!(matches!(
        other.block(Duration::from_millis(25)).await,
        Err(CacheError::LockTimeout)
    ));
    assert!(!other.release().await.unwrap());
    assert!(!other.refresh(None).await.unwrap());
    let mut restored = second.restore_lock("rebuild", "first").unwrap();
    assert!(
        restored
            .refresh(Some(Duration::from_secs(1)))
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(second.get("expiring").await.unwrap().is_none());
    assert!(!restored.isLocked().await.unwrap());
    assert!(other.get().await.unwrap());
    assert!(restored.isOwnedBy("second").await.unwrap());
    assert!(!restored.release().await.unwrap());
    assert!(
        other
            .refresh(Some(Duration::from_secs(11 * 24 * 60 * 60)))
            .await
            .unwrap()
    );
    assert!(other.release().await.unwrap());

    root.success(&["cache:forget", "binary", "memcached"]);
    assert!(second.get("binary").await.unwrap().is_none());
    let neighbor = Sandbox::new(
        "memcached-neighbor",
        &format!("CACHE_STORE=memcached\nMEMCACHED_PORT={port}\nCACHE_PREFIX=neighbor:\n"),
    );
    let neighbor = neighbor.cache(None).await;
    assert!(neighbor.forever("data", &9.into()).await.unwrap());
    root.success(&["cache:clear", "memcached"]);
    assert!(neighbor.get("data").await.unwrap().is_none());
}

#[tokio::test]
async fn memcached_weighted_servers_and_persistent_id_are_application_scoped() {
    let (Ok(first), Ok(second)) = (
        std::env::var("MEMCACHED_FIXTURE_PORT"),
        std::env::var("MEMCACHED_SECOND_PORT"),
    ) else {
        assert!(std::env::var_os("MEMCACHED_FIXTURE_REQUIRED").is_none());
        return;
    };
    let first = first.parse::<u16>().unwrap();
    let second = second.parse::<u16>().unwrap();
    let root = Sandbox::new("memcached-routing", "CACHE_STORE=array\n");
    let mut app = Application::configure(root.0.clone())
        .withConfig(config::load)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().get::<CacheManager>("cache").unwrap();
    let first_server = serde_json::json!({"host":"127.0.0.1","port":first,"weight":1});
    let second_server = serde_json::json!({"host":"127.0.0.1","port":second,"weight":4});
    let options = serde_json::json!({"driver":"memcached","persistent_id":"weighted",
        "prefix":"weighted:","servers":[first_server,second_server]});
    let weighted = manager.build(options.clone()).unwrap();
    let mut reversed = options.clone();
    reversed["servers"].as_array_mut().unwrap().reverse();
    let reordered = manager.build(reversed).unwrap();
    let mut conflict = options.clone();
    conflict["options"] = serde_json::json!({"max_connections":4});
    assert!(matches!(
        manager.build(conflict),
        Err(CacheError::InvalidConfiguration)
    ));
    let invalid = serde_json::json!({"driver":"memcached","servers":[]});
    assert!(matches!(
        manager.build(invalid),
        Err(CacheError::InvalidConfiguration)
    ));

    let only = |server: &serde_json::Value| {
        manager
            .build(serde_json::json!({
                "driver":"memcached","prefix":"weighted:","servers":[server]
            }))
            .unwrap()
    };
    let left = only(&first_server);
    let right = only(&second_server);
    weighted.flush().await.unwrap();
    let entries: Vec<_> = (0..100)
        .map(|index| (format!("item:{index}"), Value::from(index)))
        .collect();
    assert!(
        weighted
            .putMany(&entries, Some(Ttl::Seconds(300)))
            .await
            .unwrap()
    );
    let keys: Vec<_> = entries.iter().map(|(key, _)| key.as_str()).collect();
    let expected: Vec<_> = entries
        .iter()
        .map(|(_, value)| Some(value.clone()))
        .collect();
    assert_eq!(reordered.many(&keys).await.unwrap(), expected);
    let left_count = left
        .many(&keys)
        .await
        .unwrap()
        .into_iter()
        .flatten()
        .count();
    let right_count = right
        .many(&keys)
        .await
        .unwrap()
        .into_iter()
        .flatten()
        .count();
    assert_eq!(left_count + right_count, entries.len());
    assert!(left_count > 0 && right_count > left_count);
    assert!(weighted.flush().await.unwrap());
    assert!(
        left.many(&keys)
            .await
            .unwrap()
            .into_iter()
            .all(|value| value.is_none())
    );
    assert!(
        right
            .many(&keys)
            .await
            .unwrap()
            .into_iter()
            .all(|value| value.is_none())
    );
}

#[tokio::test]
async fn memcached_sasl_rejects_wrong_credentials_and_accepts_configured_credentials() {
    let Ok(port) = std::env::var("MEMCACHED_SASL_PORT") else {
        assert!(std::env::var_os("MEMCACHED_REQUIRE_SASL").is_none());
        return;
    };
    let basic_env =
        format!("CACHE_STORE=memcached\nMEMCACHED_PORT={port}\nMEMCACHED_USERNAME=fixture\n");
    let invalid = Sandbox::new(
        "memcached-invalid",
        &format!("{basic_env}MEMCACHED_PASSWORD=invalid\n"),
    );
    assert!(matches!(
        invalid.cache(None).await.get("sasl").await,
        Err(CacheError::Backend { .. })
    ));
    let valid = Sandbox::new(
        "memcached-valid",
        &format!("{basic_env}MEMCACHED_PASSWORD=local-fixture-secret\n"),
    );
    let cache = valid.cache(None).await;
    assert!(cache.forever("sasl", &"ready".into()).await.unwrap());
    assert_eq!(cache.get("sasl").await.unwrap(), Some("ready".into()));
    assert!(cache.forget("sasl").await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn memcached_unix_socket_works_with_the_configured_cache_and_artisan() {
    let Ok(path) = std::env::var("MEMCACHED_FIXTURE_SOCKET") else {
        assert!(std::env::var_os("MEMCACHED_REQUIRE_UNIX").is_none());
        return;
    };
    let root = Sandbox::new(
        "memcached-unix",
        &format!("CACHE_STORE=memcached\nMEMCACHED_HOST={path}\nMEMCACHED_PORT=0\n"),
    );
    let cache = root.cache(None).await;
    assert!(
        cache
            .put("local", &42.into(), Some(Ttl::Seconds(60)))
            .await
            .unwrap()
    );
    assert_eq!(cache.get("local").await.unwrap(), Some(42.into()));
    root.success(&["cache:forget", "local", "memcached"]);
    assert!(cache.get("local").await.unwrap().is_none());
}

#[tokio::test]
async fn dynamodb_cache_batches_expiry_counters_and_locks_use_the_real_table() {
    let Ok(endpoint) = std::env::var("DYNAMODB_FIXTURE_ENDPOINT") else {
        assert!(std::env::var_os("DYNAMODB_FIXTURE_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new(
        "dynamodb",
        &format!(
            "CACHE_STORE=dynamodb\nCACHE_PREFIX=verified:\nAWS_ACCESS_KEY_ID=fixture\nAWS_SECRET_ACCESS_KEY=localfixture\nAWS_SESSION_TOKEN=fixturetoken\nDYNAMODB_ENDPOINT={endpoint}\nDYNAMODB_CACHE_TABLE=cache\n",
        ),
    );
    let first = root.cache(None).await;
    let second = root.cache(None).await;
    assert!(first.many(&[]).await.unwrap().is_empty());
    assert!(!first.supportsTags() && !first.supportsFlushingLocks());
    assert!(matches!(
        first.flush().await,
        Err(CacheError::FlushUnsupported)
    ));

    let mut entries: Vec<_> = (0..122)
        .map(|value| (format!("key-{value}"), Value::from(value)))
        .collect();
    entries.push(("key-1".into(), 123.into()));
    entries.push(("binary".into(), vec![0, 255].into()));
    assert!(
        first
            .putMany(&entries, Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    let mut keys: Vec<_> = entries.iter().map(|(key, _)| key.as_str()).collect();
    keys.push("missing");
    let values = second.many(&keys).await.unwrap();
    assert_eq!(values[1], Some(123.into()));
    assert_eq!(values[122], Some(123.into()));
    assert_eq!(values[123], Some(vec![0, 255].into()));
    assert!(values[124].is_none());
    assert_eq!(values[121], Some(121.into()));

    let large = Value::from(serde_json::json!("\"".repeat(180_000)));
    let large_batch: Vec<_> = (0..25)
        .map(|index| (format!("escaped-{index}"), large.clone()))
        .collect();
    assert!(
        first
            .putMany(&large_batch, Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    assert_eq!(second.get("escaped-24").await.unwrap(), Some(large));

    let amount = Value::from(-2);
    let (first_add, second_add) = tokio::join!(
        first.add("race", &amount, None),
        second.add("race", &amount, None),
    );
    assert_ne!(first_add.unwrap(), second_add.unwrap());
    let (first_pull, second_pull) = tokio::join!(first.pull("race"), second.pull("race"));
    assert_ne!(
        first_pull.unwrap().is_some(),
        second_pull.unwrap().is_some()
    );

    assert!(matches!(
        first.increment("missing", 1).await,
        Err(CacheError::MissingCounter)
    ));
    assert!(first.forever("counter", &i64::MAX.into()).await.unwrap());
    assert!(matches!(
        second.increment("counter", 1).await,
        Err(CacheError::InvalidValue)
    ));
    assert_eq!(first.get("counter").await.unwrap(), Some(i64::MAX.into()));
    assert_eq!(first.decrement("counter", i64::MAX).await.unwrap(), 0);
    let (left, right) = tokio::join!(
        first.increment("counter", 2),
        second.decrement("counter", 5)
    );
    assert!(matches!(
        (left.unwrap(), right.unwrap()),
        (2, -3) | (-3, -5)
    ));
    assert_eq!(first.get("counter").await.unwrap(), Some((-3).into()));
    assert!(
        first
            .forever("decimal", &Value::from(serde_json::json!(-1.75)))
            .await
            .unwrap()
    );
    assert_eq!(first.increment("decimal", 1).await.unwrap(), 0);
    assert_eq!(
        first.get("decimal").await.unwrap(),
        Some(Value::from(serde_json::json!(-0.75)))
    );
    assert!(first.forever("text", &"007".into()).await.unwrap());
    assert!(matches!(
        first.increment("text", 1).await,
        Err(CacheError::InvalidValue)
    ));
    assert_eq!(first.get("text").await.unwrap(), Some("007".into()));

    let ttl = Ttl::Seconds(1);
    assert!(first.put("expiring", &1.into(), Some(ttl)).await.unwrap());
    assert_eq!(first.increment("expiring", 2).await.unwrap(), 3);
    assert!(first.touch("binary", Ttl::Seconds(60)).await.unwrap());
    assert!(!first.touch("missing", ttl).await.unwrap());
    let mut owner = first
        .lock("guard", Some(Duration::from_secs(1)), Some("first"))
        .unwrap();
    let mut other = second.lock("guard", None, Some("second")).unwrap();
    assert!(!owner.isLocked().await.unwrap());
    assert!(owner.get().await.unwrap());
    assert!(owner.isOwnedByCurrentProcess().await.unwrap());
    assert!(other.isOwnedBy("first").await.unwrap());
    assert!(!other.isOwnedByCurrentProcess().await.unwrap());
    assert!(!other.get().await.unwrap());
    assert!(!other.refresh(None).await.unwrap());
    assert!(!other.release().await.unwrap());
    assert!(matches!(
        other.block(Duration::from_millis(30)).await,
        Err(CacheError::LockTimeout)
    ));
    let mut restored = second.restore_lock("guard", "first").unwrap();
    assert!(
        restored
            .refresh(Some(Duration::from_secs(1)))
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert!(second.get("expiring").await.unwrap().is_none());
    assert!(!first.touch("expiring", ttl).await.unwrap());
    assert!(first.add("expiring", &7.into(), None).await.unwrap());
    assert!(!restored.isLocked().await.unwrap());
    assert!(other.get().await.unwrap());
    assert!(restored.isOwnedBy("second").await.unwrap());
    assert!(!restored.release().await.unwrap());
    assert!(other.refresh(None).await.unwrap());
    assert!(other.release().await.unwrap());
    assert!(!other.release().await.unwrap());

    root.success(&["cache:forget", "binary", "dynamodb"]);
    assert!(first.get("binary").await.unwrap().is_none());
    let clear = root.artisan(&["cache:clear", "dynamodb"]);
    assert_eq!(clear.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&clear.stderr).contains("does not support clearing"));
    assert_eq!(first.get("counter").await.unwrap(), Some((-3).into()));
}

#[tokio::test]
async fn dynamodb_cache_uses_custom_attributes_for_conditions_and_batch_keys() {
    let Ok(endpoint) = std::env::var("DYNAMODB_FIXTURE_ENDPOINT") else {
        assert!(std::env::var_os("DYNAMODB_FIXTURE_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new("dynamodb-attributes", "CACHE_STORE=array\n");
    let mut app = Application::configure(root.0.clone())
        .withConfig(config::load)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().get::<CacheManager>("cache").unwrap();
    let config = serde_json::json!({
        "driver":"dynamodb","prefix":"schema:","table":"custom_cache",
        "attributes":{"key":"cache_id","value":"encoded","expiration":"retire_at"},
        "region":"us-east-1","endpoint":endpoint,"key":"fixture","secret":"localfixture",
    });
    let cache = manager.build(config.clone()).unwrap();
    let mut invalid = config;
    invalid["attributes"]["expiration"] = "encoded".into();
    assert!(matches!(
        manager.build(invalid),
        Err(CacheError::InvalidConfiguration)
    ));
    assert!(
        cache
            .putMany(&[("one".into(), 1.into())], Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    assert_eq!(
        cache.many(&["absent", "one"]).await.unwrap(),
        vec![None, Some(1.into())]
    );
    assert!(!cache.add("one", &2.into(), None).await.unwrap());
    assert!(cache.touch("one", Ttl::Seconds(25)).await.unwrap());
    assert_eq!(cache.increment("one", 2).await.unwrap(), 3);
    let mut lock = cache.lock("owner", None, None).unwrap();
    assert!(lock.get().await.unwrap());
    assert!(lock.refresh(None).await.unwrap());
    assert!(lock.release().await.unwrap());
    assert_eq!(cache.pull("one").await.unwrap(), Some(3.into()));
}

#[tokio::test]
async fn dynamodb_cache_retries_partial_batches_and_reports_exhaustion() {
    let Ok(endpoint) = std::env::var("DYNAMODB_RETRY_ENDPOINT") else {
        assert!(std::env::var_os("DYNAMODB_FIXTURE_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new(
        "dynamodb-retry",
        &format!(
            "CACHE_STORE=dynamodb\nCACHE_PREFIX=retry:\nAWS_ACCESS_KEY_ID=fixture\nAWS_SECRET_ACCESS_KEY=localfixture\nDYNAMODB_ENDPOINT={endpoint}\nDYNAMODB_CACHE_TABLE=retry_cache\n",
        ),
    );
    let cache = root.cache(None).await;
    let ttl = Some(Ttl::Seconds(30));
    assert!(
        cache
            .putMany(
                &[("first".into(), 1.into()), ("second".into(), 2.into())],
                ttl
            )
            .await
            .unwrap()
    );
    assert_eq!(
        cache.many(&["first", "second", "absent"]).await.unwrap(),
        vec![Some(1.into()), Some(2.into()), None]
    );
    assert!(matches!(
        cache
            .putMany(&[("always-throttled".into(), 3.into())], ttl)
            .await,
        Err(CacheError::Backend { .. }),
    ));
}

async fn verify_lock_callbacks(first: &Repository, second: &Repository) {
    use openrust::{
        cache::{CacheFuture, CacheResult},
        contracts::cache::Lock,
    };
    use tokio::sync::oneshot;

    let lease = Some(Duration::from_secs(30));
    let wait = Duration::from_secs(3);
    let key = "held-during-callback";
    let mut inspector = first.lock(key, lease, None).unwrap();
    assert!(!inspector.isLocked().await.unwrap());
    assert!(inspector.isOwnedBy(None).await.unwrap());

    let result = first
        .lock(key, lease, None)
        .unwrap()
        .getWith(|| async {
            let mut contender = second.lock(key, lease, None)?;
            assert!(!contender.get().await?);
            Ok("value")
        })
        .await
        .unwrap();
    assert_eq!(result, Some("value"));

    let mut contender = second.lock(key, lease, None).unwrap();
    assert!(contender.get().await.unwrap());
    assert!(inspector.isLocked().await.unwrap());
    assert!(inspector.isOwnedBy(contender.owner()).await.unwrap());
    assert!(!inspector.isOwnedByCurrentProcess().await.unwrap());
    assert!(contender.isOwnedByCurrentProcess().await.unwrap());
    let mut called = false;
    let missed = first
        .lock(key, lease, None)
        .unwrap()
        .getWith(|| async {
            called = true;
            Ok("unexpected")
        })
        .await
        .unwrap();
    assert_eq!(missed, None);
    assert!(!called);

    let timeout = first
        .lock(key, lease, None)
        .unwrap()
        .blockWith(Duration::from_millis(30), || async {
            called = true;
            Ok(())
        })
        .await;
    assert!(matches!(timeout, Err(CacheError::LockTimeout)));
    assert!(!called);
    assert!(contender.release().await.unwrap());

    let retry_wait = Duration::from_millis(200);
    assert!(contender.get().await.unwrap());
    let candidate = first
        .lock(key, lease, None)
        .unwrap()
        .betweenBlockedAttemptsSleepFor(10)
        .unwrap();
    let release = async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        contender.release().await
    };
    let (released, retried) = tokio::join!(
        release,
        candidate.blockWith(retry_wait, || async { Ok(42) })
    );
    assert!(released.unwrap());
    assert_eq!(retried.unwrap(), 42);

    assert!(contender.get().await.unwrap());
    let mut candidate = first
        .lock(key, lease, None)
        .unwrap()
        .betweenBlockedAttemptsSleepFor(0)
        .unwrap();
    let release = async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        contender.release().await
    };
    let (released, retried) = tokio::join!(release, candidate.block(retry_wait));
    assert!(released.unwrap());
    retried.unwrap();
    assert!(candidate.release().await.unwrap());

    let failed = first
        .lock(key, lease, None)
        .unwrap()
        .blockWith(wait, || async { Err::<(), _>(CacheError::InvalidValue) })
        .await;
    assert!(matches!(failed, Err(CacheError::InvalidValue)));
    assert!(contender.get().await.unwrap());
    assert!(contender.release().await.unwrap());
    assert_eq!(
        first
            .lock(key, lease, None)
            .unwrap()
            .blockWith(wait, || async { Ok(42) })
            .await
            .unwrap(),
        42
    );

    let owned = first.lock(key, lease, None).unwrap();
    let owner = owned.owner().to_owned();
    let result = owned
        .getWith(|| async {
            assert!(second.restore_lock(key, &owner)?.release().await?);
            assert!(contender.get().await?);
            Ok("new owner")
        })
        .await
        .unwrap();
    assert_eq!(result, Some("new owner"));
    assert!(contender.release().await.unwrap());

    let (started, running) = oneshot::channel();
    let lock = first.lock(key, lease, None).unwrap();
    let task = tokio::spawn(async move {
        lock.getWith(|| async move {
            started.send(()).unwrap();
            std::future::pending::<CacheResult<()>>().await
        })
        .await
    });
    running.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    contender.block(wait).await.unwrap();
    assert!(contender.release().await.unwrap());

    let lock = first.lock(key, lease, None).unwrap();
    let panicking = tokio::spawn(async move {
        lock.getWith(|| async {
            assert!(std::hint::black_box(false), "callback panic");
            Ok(())
        })
        .await
    });
    assert!(panicking.await.unwrap_err().is_panic());
    contender.block(wait).await.unwrap();
    assert!(contender.release().await.unwrap());

    struct TestLock {
        lock: Box<dyn Lock>,
        acquired: Option<oneshot::Sender<()>>,
        response: Option<oneshot::Receiver<()>>,
        release_error: bool,
    }

    impl Lock for TestLock {
        fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
            Box::pin(async move {
                let acquired = self.lock.get().await?;
                if let Some(signal) = self.acquired.take() {
                    signal.send(()).unwrap();
                    self.response.take().unwrap().await.unwrap();
                }
                Ok(acquired)
            })
        }

        fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>> {
            self.lock.block(wait)
        }

        fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
            Box::pin(async move {
                let released = self.lock.release().await?;
                if self.release_error {
                    return Err(CacheError::InvalidOwner);
                }
                Ok(released)
            })
        }

        fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>> {
            self.lock.refresh(lease)
        }

        fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>> {
            self.lock.force_release()
        }

        fn owner(&self) -> &str {
            self.lock.owner()
        }
    }

    let (acquired, ready) = oneshot::channel();
    let (confirm, response) = oneshot::channel();
    let lock: Box<dyn Lock> = Box::new(TestLock {
        lock: first.lock(key, lease, None).unwrap(),
        acquired: Some(acquired),
        response: Some(response),
        release_error: false,
    });
    let task = tokio::spawn(async move {
        lock.getWith(|| async {
            assert!(std::hint::black_box(false), "cancelled callback ran");
            Ok(())
        })
        .await
    });
    ready.await.unwrap();
    assert!(!contender.get().await.unwrap());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    confirm.send(()).unwrap();
    contender.block(wait).await.unwrap();
    assert!(contender.release().await.unwrap());

    let lock: Box<dyn Lock> = Box::new(TestLock {
        lock: first.lock(key, lease, None).unwrap(),
        acquired: None,
        response: None,
        release_error: true,
    });
    let result = lock
        .getWith(|| async { Err::<(), _>(CacheError::InvalidValue) })
        .await;
    assert!(matches!(result, Err(CacheError::InvalidOwner)));
    assert!(contender.get().await.unwrap());
    assert!(contender.release().await.unwrap());
}

#[tokio::test]
async fn scoped_lock_callbacks_release_database_ownership() {
    let Ok(url) = std::env::var("OPENRUST_TEST_CACHE_MYSQL_URL") else {
        assert!(std::env::var_os("CACHE_CALLBACK_FIXTURE_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new(
        "mysql-callbacks",
        &format!(
            "CACHE_STORE=database\nCACHE_PREFIX=callbacks:\nDB_CONNECTION=mysql\nDB_URL={url}\n"
        ),
    );
    root.success(&["migrate"]);
    let first = root.cache(None).await;
    let second = root.cache(None).await;
    verify_lock_callbacks(&first, &second).await;
    root.success(&["migrate:reset"]);
}

#[tokio::test]
async fn scoped_lock_callbacks_release_redis_ownership() {
    let Ok(url) = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL") else {
        assert!(std::env::var_os("CACHE_CALLBACK_FIXTURE_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new(
        "redis-callbacks",
        &format!("CACHE_STORE=redis\nCACHE_PREFIX=callbacks:\nREDIS_URL={url}\n"),
    );
    let first = root.cache(None).await;
    let second = root.cache(None).await;
    verify_lock_callbacks(&first, &second).await;
}

async fn verify_cache_concurrency(first: Arc<Repository>, second: Arc<Repository>) {
    use tokio::sync::oneshot;

    let mut external = second.lock("reports1", None, None).unwrap();
    assert!(external.get().await.unwrap());
    let (started, running) = oneshot::channel();
    let (resume, held) = oneshot::channel();
    let limiter = first.funnel("reports").unwrap().limit(2).releaseAfter(30);
    let task = tokio::spawn(limiter.then(|| async move {
        started.send(()).unwrap();
        held.await.unwrap();
        Ok(42)
    }));
    running.await.unwrap();

    let attempt = || second.funnel("reports").unwrap().limit(2).block(0);
    let unused = || async { panic!("all slots are held") };
    assert!(matches!(
        attempt().then(unused).await,
        Err(CacheError::LimiterTimeout)
    ));
    assert_eq!(
        attempt()
            .thenWith(unused, |error| async move {
                assert!(matches!(error, CacheError::LimiterTimeout));
                Ok(429)
            })
            .await
            .unwrap(),
        429
    );
    assert_eq!(
        second
            .funnel("exports")
            .unwrap()
            .limit(1)
            .block(0)
            .then(|| async { Ok(1) })
            .await
            .unwrap(),
        1
    );

    let release = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(external.release().await.unwrap());
    };
    let callback = || async { Err::<(), _>(CacheError::InvalidValue) };
    let wait = second.funnel("reports").unwrap().limit(2).block(2).sleep(5);
    let (_, result) = tokio::join!(
        release,
        wait.thenWith(callback, |_| async { panic!("unexpected fallback") })
    );
    assert!(matches!(result, Err(CacheError::InvalidValue)));
    assert_eq!(
        attempt().then(|| async { Ok("reused") }).await.unwrap(),
        "reused"
    );
    resume.send(()).unwrap();
    assert_eq!(task.await.unwrap().unwrap(), 42);

    let (started, running) = oneshot::channel();
    let limiter = first.funnel("reports").unwrap().limit(1).releaseAfter(10);
    let task = tokio::spawn(limiter.then(|| async move {
        started.send(()).unwrap();
        std::future::pending::<Result<(), CacheError>>().await
    }));
    running.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    second
        .funnel("reports")
        .unwrap()
        .limit(1)
        .block(2)
        .sleep(5)
        .then(|| async { Ok(()) })
        .await
        .unwrap();

    assert_eq!(
        first
            .withoutOverlappingWith(
                "exclusive",
                || async {
                    let blocked = second
                        .withoutOverlappingWith("exclusive", unused, 10, 0, None)
                        .await;
                    assert!(matches!(blocked, Err(CacheError::LockTimeout)));
                    let mut lock = second.lock("exclusive", None, Some("fixture-owner"))?;
                    assert!(lock.isOwnedByCurrentProcess().await?);
                    Ok("done")
                },
                10,
                0,
                Some("fixture-owner")
            )
            .await
            .unwrap(),
        "done"
    );
    second
        .withoutOverlapping("exclusive", || async { Ok(()) })
        .await
        .unwrap();
}

#[tokio::test]
async fn cache_concurrency_uses_local_lock_stores() {
    let root = Sandbox::new("array-concurrency", "CACHE_STORE=array\n");
    let array = root.cache(None).await;
    verify_cache_concurrency(array.clone(), array).await;

    let root = Sandbox::new("file-concurrency", "CACHE_STORE=file\n");
    verify_cache_concurrency(root.cache(None).await, root.cache(None).await).await;
    assert!(matches!(
        root.cache(Some("session")).await.funnel("reports"),
        Err(CacheError::LocksUnsupported)
    ));
}

#[tokio::test]
async fn cache_concurrency_shares_mysql_slots_between_applications() {
    let Ok(url) = std::env::var("OPENRUST_TEST_CACHE_MYSQL_URL") else {
        assert!(std::env::var_os("CACHE_CONCURRENCY_FIXTURES_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new(
        "mysql-concurrency",
        &format!(
            "CACHE_STORE=database\nCACHE_PREFIX=concurrency:\nDB_CONNECTION=mysql\nDB_URL={url}\n"
        ),
    );
    root.success(&["migrate"]);
    verify_cache_concurrency(root.cache(None).await, root.cache(None).await).await;
    root.success(&["migrate:reset"]);
}

#[tokio::test]
async fn cache_concurrency_shares_redis_slots_between_applications() {
    let Ok(url) = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL") else {
        assert!(std::env::var_os("CACHE_CONCURRENCY_FIXTURES_REQUIRED").is_none());
        return;
    };
    let root = Sandbox::new(
        "redis-concurrency",
        &format!("CACHE_STORE=redis\nCACHE_PREFIX=concurrency:\nREDIS_URL={url}\n"),
    );
    let first = root.cache(None).await;
    let second = root.cache(None).await;
    verify_cache_concurrency(first.clone(), second.clone()).await;

    let (started, running) = tokio::sync::oneshot::channel();
    let (resume, held) = tokio::sync::oneshot::channel();
    let limiter = first.funnel("lease").unwrap().limit(1).releaseAfter(1);
    let task = tokio::spawn(limiter.then(|| async move {
        started.send(()).unwrap();
        held.await.unwrap();
        Ok(())
    }));
    running.await.unwrap();
    let mut replacement = second.lock("lease1", None, None).unwrap();
    replacement.block(Duration::from_secs(4)).await.unwrap();
    resume.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert!(replacement.isOwnedByCurrentProcess().await.unwrap());
    assert!(replacement.release().await.unwrap());
}

#[tokio::test]
async fn remember_reports_warmth_and_only_resolves_missing_non_null_values() {
    let root = Sandbox::new("remember", "CACHE_STORE=array\n");
    let cache = root.cache(None).await;
    let ttl = Some(Ttl::Seconds(10));
    cache
        .put("cached-null", &serde_json::Value::Null.into(), ttl)
        .await
        .unwrap();
    assert_eq!(
        cache
            .rememberWithWarmthFor(
                "cached-null",
                |value| {
                    assert_eq!(value, &Value::from(1));
                    ttl
                },
                async { Ok(1.into()) }
            )
            .await
            .unwrap(),
        (1.into(), false)
    );
    assert_eq!(
        cache
            .rememberWithWarmthFor("cached-null", |_| panic!("expiry already set"), async {
                panic!("already cached")
            })
            .await
            .unwrap(),
        (1.into(), true)
    );
    assert_eq!(
        cache
            .rememberForever("permanent", async { Ok(2.into()) })
            .await
            .unwrap(),
        2.into()
    );
    assert_eq!(
        cache
            .sear("permanent", async { panic!("already cached") })
            .await
            .unwrap(),
        2.into()
    );
    assert!(matches!(
        cache
            .remember("error", ttl, async { Err(CacheError::InvalidValue) })
            .await,
        Err(CacheError::InvalidValue)
    ));
    assert!(cache.missing("error").await.unwrap());
}

async fn verify_flexible(root: &Sandbox) {
    use openrust::{
        cache::FlexibleOptions,
        contracts::http::Kernel as _,
        foundation::{configuration::Routing, http::Kernel},
        http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
        routing::{Router, RoutingError},
        support::facades::Cache,
    };

    fn endpoint(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        let path = request.uri().path().to_owned();
        Box::pin(async move {
            if path == "/double" {
                Cache::flexible("report", [0, 30], || async {
                    panic!("replaced named callback")
                })
                .await?;
            }
            let status = if path == "/failure" || path == "/always" {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            };
            let always = path == "/always";
            let value = Cache::flexibleWith(
                "report",
                [Ttl::Seconds(0), Ttl::Seconds(30)],
                move || async move {
                    if path == "/refresh-error" {
                        return Err(CacheError::InvalidValue);
                    }
                    Ok(Value::from(path.as_str()))
                },
                FlexibleOptions {
                    always_defer: always,
                    ..FlexibleOptions::default()
                },
            )
            .await?;
            Response::json(status, value.as_str())
        })
    }

    fn routes(_: &Application) -> Result<Router, RoutingError> {
        let mut router = Router::new();
        for uri in [
            "/double",
            "/success",
            "/failure",
            "/always",
            "/refresh-error",
        ] {
            router.get(uri, endpoint)?;
        }
        Ok(router)
    }

    fn request(path: &str) -> Request {
        let (parts, body) = axum::http::Request::get(path)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        Request::from_parts(parts, body)
    }

    let mut kernel = Kernel::new(
        Application::configure(root.0.clone())
            .withConfig(config::load)
            .withRouting(Routing::new(routes))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let cache = kernel
        .get_application()
        .container()
        .get::<CacheManager>("cache")
        .unwrap()
        .store(None)
        .unwrap();
    assert_eq!(
        cache
            .flexible("report", [5, 30], || async { Ok("seed".into()) })
            .await
            .unwrap(),
        "seed".into()
    );
    assert_eq!(
        cache
            .flexible("report", [5, 30], || async { panic!("fresh cache") })
            .await
            .unwrap(),
        "seed".into()
    );

    let mut first = request("/double");
    let mut second = request("/success");
    let first_response = kernel.handle(&mut first).await;
    let second_response = kernel.handle(&mut second).await;
    assert_eq!(first_response.status(), StatusCode::OK);
    assert_eq!(second_response.status(), StatusCode::OK);
    assert_eq!(cache.get("report").await.unwrap(), Some("seed".into()));
    kernel
        .terminate(first.parts(), first_response.parts())
        .await;
    kernel
        .terminate(second.parts(), second_response.parts())
        .await;
    assert_eq!(cache.get("report").await.unwrap(), Some("/double".into()));

    for (path, status, expected) in [
        ("/failure", StatusCode::BAD_REQUEST, "/double"),
        ("/always", StatusCode::BAD_REQUEST, "/always"),
        ("/refresh-error", StatusCode::OK, "/always"),
        ("/success", StatusCode::OK, "/success"),
    ] {
        let mut request = request(path);
        let response = kernel.handle(&mut request).await;
        assert_eq!(response.status(), status);
        kernel.terminate(request.parts(), response.parts()).await;
        assert_eq!(cache.get("report").await.unwrap(), Some(expected.into()));
    }
    assert_eq!(
        cache
            .flexible("short", [0, 1], || async { Ok("initial".into()) })
            .await
            .unwrap(),
        "initial".into()
    );
    assert_eq!(
        cache
            .flexible("short", [0, 1], || async {
                panic!("no active deferred scope")
            })
            .await
            .unwrap(),
        "initial".into()
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        cache
            .flexible("short", [0, 1], || async { Ok("regenerated".into()) })
            .await
            .unwrap(),
        "regenerated".into()
    );
}

#[tokio::test]
async fn flexible_refresh_respects_lifecycle_and_generation() {
    let root = Sandbox::new("flexible-array", "CACHE_STORE=array\n");
    verify_flexible(&root).await;

    if let Ok(url) = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL") {
        let root = Sandbox::new(
            "flexible-redis",
            &format!("CACHE_STORE=redis\nCACHE_PREFIX=flexible:\nREDIS_URL={url}\n"),
        );
        verify_flexible(&root).await;
    } else {
        assert!(std::env::var_os("CACHE_FLEXIBLE_FIXTURES_REQUIRED").is_none());
    }

    if let Ok(url) = std::env::var("OPENRUST_TEST_CACHE_MYSQL_URL") {
        let root = Sandbox::new(
            "flexible-mysql",
            &format!(
                "CACHE_STORE=database\nCACHE_PREFIX=flexible:\nDB_CONNECTION=mysql\nDB_URL={url}\n"
            ),
        );
        root.success(&["migrate"]);
        verify_flexible(&root).await;
        root.success(&["migrate:reset"]);
    } else {
        assert!(std::env::var_os("CACHE_FLEXIBLE_FIXTURES_REQUIRED").is_none());
    }
}

async fn verify_cache_defaults(first: &Repository, second: &Repository) {
    let null = Value::from(serde_json::Value::Null);
    first.forever("null", &null).await.unwrap();
    assert_eq!(
        first.getOr("null", "default".into()).await.unwrap(),
        "default".into()
    );
    assert_eq!(first.get("null").await.unwrap(), Some(null));
    assert_eq!(
        first
            .getOrElse("missing", || async { Ok("computed".into()) })
            .await
            .unwrap(),
        "computed".into()
    );
    assert!(first.missing("missing").await.unwrap());
    first.forever("zero", &0.into()).await.unwrap();
    assert_eq!(
        first
            .getOrElse("zero", || async { panic!("fallback on a hit") })
            .await
            .unwrap(),
        0.into()
    );
    let (left, right) = tokio::join!(
        first.pullOr("zero", "empty".into()),
        second.pullOrElse("zero", || async { Ok("empty".into()) }),
    );
    let values = [left.unwrap(), right.unwrap()];
    assert!(values.contains(&0.into()));
    assert!(values.contains(&"empty".into()));
    assert_eq!(first.get("zero").await.unwrap(), None);
    assert!(matches!(
        first
            .pullOrElse("null", || async { Err(CacheError::InvalidValue) })
            .await,
        Err(CacheError::InvalidValue)
    ));
    assert_eq!(first.get("null").await.unwrap(), None);
}

#[tokio::test]
async fn cache_defaults_keep_atomic_pull_and_propagate_backend_errors() {
    let root = Sandbox::new("defaults-array", "CACHE_STORE=array\n");
    let array = root.cache(None).await;
    verify_cache_defaults(&array, &array).await;
    assert!(matches!(
        root.cache(Some("session"))
            .await
            .getOrElse("key", || async { panic!("fallback on a backend error") })
            .await,
        Err(CacheError::SessionUnavailable)
    ));

    let root = Sandbox::new("defaults-file", "CACHE_STORE=file\n");
    verify_cache_defaults(
        root.cache(None).await.as_ref(),
        root.cache(None).await.as_ref(),
    )
    .await;

    if let Ok(url) = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL") {
        let root = Sandbox::new(
            "defaults-redis",
            &format!("CACHE_STORE=redis\nCACHE_PREFIX=defaults:\nREDIS_URL={url}\n"),
        );
        verify_cache_defaults(
            root.cache(None).await.as_ref(),
            root.cache(None).await.as_ref(),
        )
        .await;
    } else {
        assert!(std::env::var_os("CACHE_DEFAULT_FIXTURES_REQUIRED").is_none());
    }

    if let Ok(url) = std::env::var("OPENRUST_TEST_CACHE_MYSQL_URL") {
        let root = Sandbox::new(
            "defaults-mysql",
            &format!(
                "CACHE_STORE=database\nCACHE_PREFIX=defaults:\nDB_CONNECTION=mysql\nDB_URL={url}\n"
            ),
        );
        root.success(&["migrate"]);
        verify_cache_defaults(
            root.cache(None).await.as_ref(),
            root.cache(None).await.as_ref(),
        )
        .await;
        root.success(&["migrate:reset"]);
    } else {
        assert!(std::env::var_os("CACHE_DEFAULT_FIXTURES_REQUIRED").is_none());
    }
}

#[tokio::test]
async fn cache_helper_resolves_the_current_application_and_writes_first_entry() {
    use openrust::{
        cache,
        contracts::http::Kernel as _,
        foundation::{configuration::Routing, http::Kernel},
        http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
        routing::{Router, RoutingError},
    };

    fn endpoint(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            assert!(Arc::ptr_eq(&cache!()?, &openrust::support::cache()?));
            if request.uri().path() == "/write" {
                assert!(cache!({"read" => Value::from("ready"), "ignored" => Value::from("ignored")}, None).await?);
                assert_eq!(cache!("ignored").await?, None);
            }
            let value = cache!("read", Value::from("missing")).await?;
            Response::json(
                if value.as_str() == Some("ready") {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                value.as_str(),
            )
        })
    }

    fn routes(_: &Application) -> Result<Router, RoutingError> {
        let mut router = Router::new();
        router.get("/write", endpoint)?;
        router.get("/read", endpoint)?;
        Ok(router)
    }

    async fn send(kernel: &Kernel, path: &str) -> StatusCode {
        let (parts, body) = axum::http::Request::get(path)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        kernel
            .handle(&mut Request::from_parts(parts, body))
            .await
            .status()
    }

    assert!(matches!(cache!(), Err(CacheError::Unavailable(_))));
    let root = Sandbox::new("helper-scope", "CACHE_STORE=array\n");
    let application = || {
        Kernel::new(
            Application::configure(root.0.clone())
                .withConfig(config::load)
                .withRouting(Routing::new(routes))
                .create()
                .unwrap(),
        )
    };
    let mut first = application();
    let mut second = application();
    first.bootstrap().await.unwrap();
    second.bootstrap().await.unwrap();
    assert_eq!(send(&first, "/write").await, StatusCode::OK);
    assert_eq!(send(&second, "/read").await, StatusCode::NOT_FOUND);
    assert_eq!(send(&first, "/read").await, StatusCode::OK);
}

async fn verify_typed_and_batch(cache: &Repository) {
    use serde_json::json;

    let input = [
        ("title".into(), "Rust".into()),
        ("count".into(), " +12 ".into()),
        ("zero".into(), Value::from(0)),
        ("ratio".into(), ".5e1".into()),
        ("listed".into(), json!(false).into()),
        ("tags".into(), json!(["news"]).into()),
        ("indexed".into(), json!({"topic":"news"}).into()),
        ("null".into(), json!(null).into()),
        ("padded".into(), "012".into()),
        ("ratio-underflow".into(), "1e-600".into()),
    ];
    assert!(
        cache
            .setMultiple(&input, Some(Ttl::Seconds(30)))
            .await
            .unwrap()
    );
    assert_eq!(cache.string("title", None).await.unwrap(), "Rust");
    assert_eq!(cache.integer("count", None).await.unwrap(), 12);
    assert_eq!(cache.float("ratio", None).await.unwrap(), 5.0);
    assert!(!cache.boolean("listed", None).await.unwrap());
    assert_eq!(cache.array("tags", None).await.unwrap(), json!(["news"]));
    assert_eq!(
        cache.array("indexed", None).await.unwrap(),
        json!({"topic":"news"})
    );
    assert_eq!(
        cache
            .stringOrElse("null", || async { Ok("fallback".into()) })
            .await
            .unwrap(),
        "fallback"
    );
    assert_eq!(
        cache
            .integerOrElse("count", || async { panic!("cached value") })
            .await
            .unwrap(),
        12
    );
    assert!(
        cache
            .boolean("missing", Some(json!(true).into()))
            .await
            .unwrap()
    );
    assert!(matches!(
        cache.integer("padded", None).await,
        Err(CacheError::InvalidType {
            expected: "integer",
            ..
        })
    ));
    assert!(matches!(
        cache.integer("ratio", None).await,
        Err(CacheError::InvalidType { .. })
    ));
    assert!(matches!(
        cache.float("ratio-underflow", None).await,
        Err(CacheError::InvalidType {
            expected: "float",
            ..
        })
    ));
    assert!(
        matches!(cache.string("zero", Some("fallback".into())).await, Err(CacheError::InvalidType { key, expected: "string" }) if key == "zero")
    );
    assert!(matches!(
        cache.array("missing", None).await,
        Err(CacheError::InvalidType { .. })
    ));

    let multiple = cache
        .getMultiple(&["count", "null", "missing", "count"], Some(7.into()))
        .await
        .unwrap();
    assert_eq!(
        multiple.keys().map(String::as_str).collect::<Vec<_>>(),
        ["count", "null", "missing"]
    );
    assert_eq!(multiple["count"], Some(" +12 ".into()));
    assert_eq!(multiple["null"], Some(7.into()));
    assert_eq!(multiple["missing"], Some(7.into()));
    let without_default = cache.getMultiple(&["null", "missing"], None).await.unwrap();
    assert_eq!(without_default["null"], None);
    assert_eq!(without_default["missing"], None);
    let separate = cache
        .manyOr(&[
            ("null", 2.into()),
            ("missing", 3.into()),
            ("null", 4.into()),
        ])
        .await
        .unwrap();
    assert_eq!(separate["null"], 4.into());
    assert_eq!(separate["missing"], 3.into());
    let lazy = cache
        .manyOrElse(&["zero", "null", "missing", "null"], |key| async move {
            assert_ne!(key, "zero");
            Ok(Value::from(key))
        })
        .await
        .unwrap();
    assert_eq!(lazy["zero"], 0.into());
    assert_eq!(lazy["null"], "null".into());
    assert_eq!(lazy["missing"], "missing".into());
    assert!(cache.missing("missing").await.unwrap());

    assert!(cache.set("delete", &1.into(), None).await.unwrap());
    assert!(cache.delete("delete").await.unwrap());
    assert!(
        !cache
            .deleteMultiple(&["missing", "title", "zero"])
            .await
            .unwrap()
    );
    assert!(cache.missing("title").await.unwrap());
    assert!(cache.missing("zero").await.unwrap());
    assert!(
        !cache
            .setMultiple(&input, Some(Ttl::Seconds(0)))
            .await
            .unwrap()
    );
    assert!(cache.missing("count").await.unwrap());
    assert!(cache.set("clear", &1.into(), None).await.unwrap());
    assert!(cache.clear().await.unwrap());
    assert!(cache.missing("clear").await.unwrap());
}

#[tokio::test]
async fn cache_typed_and_batch_methods_keep_values_defaults_and_deletion_semantics() {
    for driver in ["array", "file"] {
        let root = Sandbox::new(
            &format!("typed-{driver}"),
            &format!("CACHE_STORE={driver}\n"),
        );
        verify_typed_and_batch(root.cache(None).await.as_ref()).await;
    }

    if let Ok(url) = std::env::var("OPENRUST_TEST_FAILOVER_REDIS_URL") {
        let root = Sandbox::new(
            "typed-redis",
            &format!(
                "CACHE_STORE=redis\nCACHE_PREFIX=typed:\nREDIS_CACHE_DB=11\nREDIS_URL={url}\n"
            ),
        );
        let cache = root.cache(None).await;
        verify_typed_and_batch(&cache).await;
        let tagged = cache.tags(&["typed"]).unwrap();
        for repo in [cache.as_ref(), &*tagged] {
            let ttl = Some(Ttl::Seconds(30));
            repo.put("numeric-text", &" +12 ".into(), ttl)
                .await
                .unwrap();
            assert_eq!(repo.string("numeric-text", None).await.unwrap(), " +12 ");
            assert_eq!(repo.increment("numeric-text", 1).await.unwrap(), 13);
            repo.put("overflow-text", &"9223372036854775807".into(), ttl)
                .await
                .unwrap();
            assert!(repo.increment("overflow-text", 1).await.is_err());
            assert_eq!(
                repo.string("overflow-text", None).await.unwrap(),
                "9223372036854775807"
            );
            assert_eq!(
                repo.decrement("overflow-text", 1).await.unwrap(),
                i64::MAX - 1
            );
        }
    } else {
        assert!(std::env::var_os("CACHE_TYPED_FIXTURES_REQUIRED").is_none());
    }

    if let Ok(url) = std::env::var("OPENRUST_TEST_CACHE_MYSQL_URL") {
        let root = Sandbox::new(
            "typed-mysql",
            &format!(
                "CACHE_STORE=database\nCACHE_PREFIX=typed:\nDB_CONNECTION=mysql\nDB_URL={url}\n"
            ),
        );
        root.success(&["migrate"]);
        verify_typed_and_batch(root.cache(None).await.as_ref()).await;
        root.success(&["migrate:reset"]);
    } else {
        assert!(std::env::var_os("CACHE_TYPED_FIXTURES_REQUIRED").is_none());
    }
}
