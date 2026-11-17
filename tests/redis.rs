use std::{
    fs, net::SocketAddr, num::NonZeroU64, path::PathBuf, process::Command, sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::{CacheError, Ttl, Value},
    contracts::{
        cache::{LockProvider as _, Repository as _},
        container::Container as _,
        foundation::Application as _,
        http::Kernel as _,
        redis::Connection as _,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    redis::RedisManager,
    support::facades::{Cache, Redis, Route},
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[path = "../config/mod.rs"]
mod config;

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn application(path: PathBuf) -> Application {
    Application::configure(path)
        .withConfig(|base, environment| {
            let mut config = config::load(base, environment)?;
            config.get_mut("app").unwrap()["key"] = "a".repeat(32).into();
            config.get_mut("session").unwrap()["driver"] = "array".into();
            Ok(config)
        })
        .withRouting(Routing::default().web(|| Route::get("/cache", cache_http)))
        .create()
        .unwrap()
}

fn cache_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let value = Cache::get("reports/latest")
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        let reply = value.as_ref().and_then(Value::as_str).unwrap_or("empty");
        let _: openrust::redis::Value =
            Redis::command("PING", &[])
                .await
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
        Ok(Response::new(StatusCode::OK, Body::from(reply.to_owned())))
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let (parts, body) = kernel
        .handle(&mut Request::from_parts(parts, body))
        .await
        .into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn request(address: SocketAddr) -> String {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET /cache HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    response.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn redis_lock_child() {
    let Some(path) = std::env::var_os("OPENRUST_REDIS_LOCK_TEST_ROOT") else {
        return;
    };
    let mut app = application(path.into());
    app.boot().await.unwrap();
    let cache = app
        .container()
        .get::<openrust::cache::CacheManager>("cache")
        .unwrap();
    let repository = cache.store(None).unwrap();
    let owner = std::env::var("OPENRUST_REDIS_LOCK_TEST_OWNER").unwrap();
    let mut restored = repository.restore_lock("cross-process", &owner).unwrap();
    assert!(restored.release().await.unwrap());
}

#[tokio::test]
async fn named_redis_connections_cache_and_locks_work_across_entrypoints() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") else {
        return;
    };
    let root =
        Root(std::env::temp_dir().join(format!("openrust-redis-test-{}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::write(
        root.0.join(".env"),
        format!(
            "APP_ENV=local\nCACHE_STORE=redis\nREDIS_URL=redis://127.0.0.1:{port}\nREDIS_PREFIX=probe:\nCACHE_PREFIX=cache:\nREDIS_DB=0\nREDIS_CACHE_DB=1\n"
        ),
    ).unwrap();
    let mut kernel = Kernel::new(application(root.0.clone()));
    kernel.bootstrap().await.unwrap();
    let manager = kernel
        .get_application()
        .container()
        .get::<RedisManager>("redis")
        .unwrap();
    let default = manager.connection(None).unwrap();
    let data = manager.connection(Some("cache")).unwrap();
    assert!(Arc::ptr_eq(
        &default,
        &manager.connection(Some("default")).unwrap()
    ));
    assert!(manager.connection(Some("missing")).is_err());
    default.set("sentinel", b"keep").await.unwrap();
    assert_eq!(data.get("sentinel").await.unwrap(), None);

    default
        .command(
            "ACL",
            &[
                b"SETUSER",
                b"stage-user",
                b"on",
                b">stage-only-password",
                b"~*",
                b"+@all",
            ],
        )
        .await
        .unwrap();
    let auth = root.0.join("auth");
    fs::create_dir(&auth).unwrap();
    fs::write(auth.join(".env"), format!(
        "REDIS_URL=redis://127.0.0.1:{port}/2\nREDIS_USERNAME=stage-user\nREDIS_PASSWORD=stage-only-password\n"
    )).unwrap();
    let mut authenticated = application(auth);
    authenticated.boot().await.unwrap();
    let authenticated_redis = authenticated
        .container()
        .get::<RedisManager>("redis")
        .unwrap();
    let authenticated = authenticated_redis.connection(None).unwrap();
    authenticated.set("authenticated", b"works").await.unwrap();
    assert_eq!(
        authenticated.command("ACL", &[b"WHOAMI"]).await.unwrap(),
        openrust::redis::Value::BulkString(b"stage-user".to_vec())
    );
    assert_eq!(default.get("authenticated").await.unwrap(), None);
    default
        .command("ACL", &[b"DELUSER", b"stage-user"])
        .await
        .unwrap();

    let mut subscription = default.subscribe(&["probe-channel"]).await.unwrap();
    default
        .command("PUBLISH", &[b"probe-channel", b"hello"])
        .await
        .unwrap();
    let message = tokio::time::timeout(Duration::from_secs(2), subscription.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.get_payload_bytes(), b"hello");

    let cache = kernel
        .get_application()
        .container()
        .get::<openrust::cache::CacheManager>("cache")
        .unwrap();
    let repository = cache.store(None).unwrap();
    repository
        .forever("reports/latest", &Value::from("ready"))
        .await
        .unwrap();
    assert!(data.get("cache:reports/latest").await.unwrap().is_some());
    assert!(default.get("cache:reports/latest").await.unwrap().is_none());

    let bytes = Value::Bytes(vec![0, 255]);
    let store = repository.get_store();
    assert!(
        store
            .put_many(
                &[
                    ("batch-json".to_owned(), Value::from("batch")),
                    ("batch-bytes".to_owned(), bytes.clone())
                ],
                NonZeroU64::new(5).unwrap(),
            )
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .many(&["batch-json", "batch-miss", "batch-bytes"])
            .await
            .unwrap(),
        vec![Some(Value::from("batch")), None, Some(bytes)]
    );

    let mut independent = application(root.0.clone());
    independent.boot().await.unwrap();
    let independent_manager = independent
        .container()
        .get::<RedisManager>("redis")
        .unwrap();
    let independent_data = independent_manager.connection(Some("cache")).unwrap();
    assert!(!Arc::ptr_eq(&data, &independent_data));
    let independent_cache = independent
        .container()
        .get::<openrust::cache::CacheManager>("cache")
        .unwrap();
    let second = independent_cache.store(None).unwrap();
    let race_value = Value::from("race");
    let ttl = Some(Ttl::Seconds(5));
    let (a, b) = tokio::join!(
        repository.add("race", &race_value, ttl),
        second.add("race", &race_value, ttl)
    );
    assert_ne!(a.unwrap(), b.unwrap());
    let (a, b) = tokio::join!(repository.pull("race"), second.pull("race"));
    assert_ne!(a.unwrap().is_some(), b.unwrap().is_some());

    repository
        .put("counter", &Value::from(1), ttl)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        repository.increment("counter", 1),
        second.increment("counter", 1)
    );
    assert_ne!(a.unwrap(), b.unwrap());
    assert_eq!(
        repository.get("counter").await.unwrap(),
        Some(Value::from(3))
    );
    let counter = data.key("cache:counter");
    assert!(matches!(
        data.command("PTTL", &[counter.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(milliseconds) if milliseconds > 0
    ));
    assert!(repository.touch("counter", Ttl::Seconds(1)).await.unwrap());
    let stored = " +12 ";
    repository
        .put("text-counter", &Value::from(stored), ttl)
        .await
        .unwrap();
    assert_eq!(
        repository.string("text-counter", None).await.unwrap(),
        stored
    );
    assert_eq!(second.increment("text-counter", 1).await.unwrap(), 13);
    assert_eq!(
        repository.get("text-counter").await.unwrap(),
        Some(Value::from(13))
    );
    let key = data.key("cache:text-counter");
    assert!(matches!(
        data.command("PTTL", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(milliseconds) if milliseconds > 0
    ));

    let mut held = repository
        .lock("lease", Some(Duration::from_secs(5)), None)
        .unwrap();
    let mut contender = second
        .lock("lease", Some(Duration::from_secs(5)), None)
        .unwrap();
    assert!(held.get().await.unwrap());
    assert!(default.get("cache:lease").await.unwrap().is_some());
    assert!(data.get("cache:lease").await.unwrap().is_none());
    assert!(matches!(
        contender.block(Duration::from_millis(30)).await,
        Err(CacheError::LockTimeout)
    ));
    assert!(
        held.refresh(Some(Duration::from_millis(100)))
            .await
            .unwrap()
    );
    contender.block(Duration::from_secs(2)).await.unwrap();
    assert!(!held.release().await.unwrap());
    assert!(!held.refresh(None).await.unwrap());
    assert!(contender.release().await.unwrap());

    let mut held = repository
        .lock("cross-process", Some(Duration::from_secs(10)), None)
        .unwrap();
    assert!(held.get().await.unwrap());
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "redis_lock_child", "--nocapture"])
        .env("OPENRUST_REDIS_LOCK_TEST_ROOT", &root.0)
        .env("OPENRUST_REDIS_LOCK_TEST_OWNER", held.owner())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(!held.release().await.unwrap());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    assert_eq!(request(address).await, "ready");

    // The manager reconnects after a lost transport. Callers decide whether a failed command is safe to retry.
    let openrust::redis::Value::Int(client_id) = data.command("CLIENT", &[b"ID"]).await.unwrap()
    else {
        panic!("expected a numeric Redis client ID");
    };
    let client_id = client_id.to_string();
    independent_data
        .command("CLIENT", &[b"KILL", b"ID", client_id.as_bytes()])
        .await
        .unwrap();
    let recovered = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Ok(Some(value)) = repository.get("reports/latest").await {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(recovered, Value::from("ready"));

    let mut surviving = repository.lock("survives-flush", None, None).unwrap();
    assert!(surviving.get().await.unwrap());
    let artisan = Command::new(env!("CARGO_BIN_EXE_main"))
        .args(["artisan", "cache:clear", "redis"])
        .env("APP_BASE_PATH", &root.0)
        .output()
        .unwrap();
    assert!(
        artisan.status.success(),
        "{}",
        String::from_utf8_lossy(&artisan.stderr)
    );
    assert_eq!(request(address).await, "empty");
    assert_eq!(default.get("sentinel").await.unwrap().unwrap(), b"keep");
    assert!(surviving.release().await.unwrap());
    server.abort();
}

#[tokio::test]
async fn redis_cluster_routes_commands_and_cache_locks_across_shards() {
    use openrust::{cache::CacheManager, redis::RedisError};
    use serde_json::json;

    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_CLUSTER_PORT") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS_CLUSTER").is_none());
        return;
    };
    let root =
        Root(std::env::temp_dir().join(format!("openrust-redis-cluster-{}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::write(root.0.join(".env"), "CACHE_STORE=redis\nREDIS_CACHE_CONNECTION=sharded\nREDIS_CACHE_LOCK_CONNECTION=sharded\nCACHE_PREFIX=cache:\nREDIS_PREFIX=fixture:\n").unwrap();
    let app = || {
        Application::configure(root.0.clone()).withConfig(|base, env| {
        let mut config = config::load(base, env)?;
        let cluster_port: u16 = std::env::var("OPENRUST_TEST_REDIS_CLUSTER_PORT").unwrap().parse().unwrap();
        config.get_mut("database").unwrap()["redis"]["clusters"] = json!({
            "sharded": [{"host":"127.0.0.1","port":1,"password":"cluster-fixture"},{"url":format!("redis://:cluster-fixture@127.0.0.1:{cluster_port}/0")}],
            "invalid": [{"port":cluster_port,"database":1}],
            "invalid_url": [{"url":format!("redis://127.0.0.1:{cluster_port}/2")}],
            "overflow": [{"port":cluster_port,"max_retries":100}],
            "empty": [],
            "default": []
        });
        Ok(config)
    }).create().unwrap()
    };
    let mut first = app();
    let mut second = app();
    first.boot().await.unwrap();
    second.boot().await.unwrap();
    let manager = first.container().get::<RedisManager>("redis").unwrap();
    for invalid in ["invalid", "invalid_url", "overflow", "empty"] {
        assert!(matches!(
            manager.connection(Some(invalid)),
            Err(RedisError::InvalidConfiguration)
        ));
    }
    assert!(manager.connection(None).is_ok()); // A direct definition precedes the same cluster name.
    let connection = manager.connection(Some("sharded")).unwrap();
    assert!(Arc::ptr_eq(
        &connection,
        &manager.connection(Some("sharded")).unwrap()
    ));
    assert_eq!(connection.key("{one}:report"), "fixture:{one}:report");

    // Only one live seed is supplied. Redis must discover the other primary nodes.
    let status = connection.command("CLUSTER", &[b"INFO"]).await.unwrap();
    assert!(
        format!("{status:?}").contains("cluster_state:ok"),
        "{status:?} from seed {port}"
    );
    for key in ["{one}:report", "{two}:report", "{three}:report"] {
        connection.set(key, key.as_bytes()).await.unwrap();
        assert_eq!(
            connection.get(key).await.unwrap(),
            Some(key.as_bytes().to_vec())
        );
    }
    let mut subscription = connection.subscribe(&["fixture-events"]).await.unwrap();
    connection
        .command("PUBLISH", &[b"fixture-events", b"ready"])
        .await
        .unwrap();
    let message = tokio::time::timeout(Duration::from_secs(3), subscription.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.get_payload::<String>().unwrap(), "ready");
    let mut patterns = connection.psubscribe(&["fixture-*"]).await.unwrap();
    connection
        .command("PUBLISH", &[b"fixture-pattern", b"matched"])
        .await
        .unwrap();
    let message = tokio::time::timeout(Duration::from_secs(3), patterns.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.get_payload::<String>().unwrap(), "matched");

    let repository = first
        .container()
        .get::<CacheManager>("cache")
        .unwrap()
        .store(None)
        .unwrap();
    let peer = second
        .container()
        .get::<CacheManager>("cache")
        .unwrap()
        .store(None)
        .unwrap();
    let ttl = Some(Ttl::Seconds(15));
    let left = "{one}:cache";
    let right = "{two}:cache";
    assert!(
        repository
            .putMany(
                &[(left.into(), "left".into()), (right.into(), "right".into())],
                ttl
            )
            .await
            .unwrap()
    );
    assert_eq!(
        peer.many(&[right, "{three}:missing", left, right])
            .await
            .unwrap(),
        [
            Some("right".into()),
            None,
            Some("left".into()),
            Some("right".into())
        ]
    );
    let (left_pull, right_pull) = tokio::join!(repository.pull(left), peer.pull(left));
    assert_ne!(left_pull.unwrap().is_some(), right_pull.unwrap().is_some());

    let lease = Some(Duration::from_secs(3));
    let mut owner = repository
        .lock("{three}:lock", lease, Some("fixture-owner"))
        .unwrap();
    let mut other = peer.lock("{three}:lock", lease, None).unwrap();
    assert!(owner.get().await.unwrap());
    assert!(!other.get().await.unwrap());
    assert!(!other.release().await.unwrap());
    assert!(owner.refresh(lease).await.unwrap());
    assert!(
        peer.restore_lock("{three}:lock", owner.owner())
            .unwrap()
            .release()
            .await
            .unwrap()
    );
    assert!(other.get().await.unwrap());
    assert!(other.release().await.unwrap());
    assert!(matches!(
        repository.flushLocks().await,
        Err(CacheError::LockFlushUnsupported)
    ));
    assert!(matches!(
        repository.tags(&["reports"]),
        Err(CacheError::TagsUnsupported)
    ));
    assert!(repository.flush().await.unwrap());
    assert_eq!(peer.get(right).await.unwrap(), None);
    for key in ["{one}:report", "{two}:report", "{three}:report"] {
        assert_eq!(connection.get(key).await.unwrap(), None);
    }
}
