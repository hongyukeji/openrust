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

    async fn tags_action(action: &str) -> Result<Option<Value>, CacheError> {
        match action {
            "/write" => {
                assert!(store_report().await?);
                Ok(None)
            }
            "/flush" => {
                Cache::tags(&["daily"])?.flush().await?;
                Ok(None)
            }
            _ => Cache::tags(&["reports", "daily"])?.get("latest").await,
        }
    }

    fn endpoint(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let result =
                tags_action(request.uri().path())
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
            Response::json(
                StatusCode::OK,
                result.and_then(|value| value.as_str().map(str::to_owned)),
            )
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
    let root = Sandbox::new("tags-http", &env);
    let mut kernel = Kernel::new(
        Application::configure(root.0.clone())
            .withConfig(config::load)
            .withRouting(Routing::new(routes))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

    timeout(Duration::from_secs(5), async {
        for (method, path, expected) in [
            ("GET", "/read", "null"),
            ("POST", "/write", "null"),
            ("GET", "/read", "\"ready\""),
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
    root.success(&["inspire"]);
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
