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

    let mut first = cache.lock("no-lock", None, None).unwrap();
    let mut second = cache.lock("no-lock", None, None).unwrap();
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
