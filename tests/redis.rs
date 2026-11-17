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
        Err(CacheError::LockStoreNotSeparate)
    ));
    assert!(repository.supportsTags());
    assert!(repository.flush().await.unwrap());
    assert_eq!(peer.get(right).await.unwrap(), None);
    for key in ["{one}:report", "{two}:report", "{three}:report"] {
        assert_eq!(connection.get(key).await.unwrap(), None);
    }
}

fn cluster_cli(container: &str, port: &str, args: &[&str]) -> String {
    let result = Command::new("docker")
        .args(["exec", "-e", "REDISCLI_AUTH=cluster-fixture", container])
        .args(["redis-cli", "--raw", "-p", port])
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success(), "{args:?}: {result:?}");
    String::from_utf8(result.stdout).unwrap().trim().into()
}

fn cluster_replica_read(container: &str, port: &str, key: &str) -> String {
    use std::{io::Write as _, process::Stdio};

    let mut client = Command::new("docker")
        .args([
            "exec",
            "-i",
            "-e",
            "REDISCLI_AUTH=cluster-fixture",
            container,
        ])
        .args(["redis-cli", "--raw", "-p", port])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(client.stdin.take().unwrap(), "READONLY\nGET {key}").unwrap();
    let result = client.wait_with_output().unwrap();
    assert!(result.status.success(), "{result:?}");
    String::from_utf8(result.stdout).unwrap().trim().into()
}

#[tokio::test]
async fn redis_cluster_migrates_and_recovers_without_replaying_a_write() {
    use openrust::{cache::RedisStore, contracts::cache::Store as _};
    use serde_json::json;

    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_CLUSTER_PORT") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS_CLUSTER").is_none());
        return;
    };
    let container = std::env::var("OPENRUST_TEST_REDIS_CLUSTER_CONTAINER").unwrap();
    let directory = std::env::var_os("OPENRUST_TEST_REDIS_CLUSTER_FIXTURE").unwrap();
    let seed = json!({"host":"127.0.0.1","port":port.parse::<u16>().unwrap(),"password":"cluster-fixture"});
    let mut without_redirects = seed.clone();
    without_redirects["max_retries"] = json!(0);
    let mut restricted_seed = seed.clone();
    restricted_seed["username"] = json!("fixture-limited");
    restricted_seed["password"] = json!("fixture-limited-password");
    let manager = RedisManager::new(Arc::new(openrust::config::Repository::new(
        [(
            "database".into(),
            json!({"redis":{"options":{"prefix":"fixture:"},"clusters":{
                "topology":[seed],
                "no_redirects":[without_redirects],
                "restricted":[restricted_seed],
            }}}),
        )]
        .into(),
    )));
    let connection = manager.connection(Some("topology")).unwrap();

    // The server increments the key. The relay drops its acknowledgment once.
    let lost_reply = "{two}:lost-reply";
    let physical = connection.key(lost_reply);
    connection.set(lost_reply, b"0").await.unwrap();
    assert_eq!(cluster_cli(&container, &port, &["GET", &physical]), "0");
    assert!(
        connection
            .command("INCR", &[physical.as_bytes()])
            .await
            .is_err()
    );
    assert!(
        PathBuf::from(directory)
            .join("acknowledgment-dropped")
            .is_file()
    );
    assert_eq!(cluster_cli(&container, &port, &["GET", &physical]), "1");

    let nodes = cluster_cli(&container, &port, &["CLUSTER", "NODES"]);
    let nodes: Vec<Vec<&str>> = nodes
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();
    let node_port = |address: &str| {
        address
            .split('@')
            .next()
            .unwrap()
            .rsplit_once(':')
            .unwrap()
            .1
            .to_owned()
    };
    for node in &nodes {
        assert_eq!(
            cluster_cli(
                &container,
                &node_port(node[1]),
                &[
                    "ACL",
                    "SETUSER",
                    "fixture-limited",
                    "on",
                    ">fixture-limited-password",
                    "~*",
                    "+@all",
                    "-command|getkeys"
                ]
            ),
            "OK"
        );
    }
    let is_primary = |node: &&Vec<&str>| node[2].split(',').any(|flag| flag == "master");
    let source = nodes
        .iter()
        .filter(is_primary)
        .find(|node| node.contains(&"5461-10922"))
        .unwrap();
    let destination = nodes
        .iter()
        .filter(is_primary)
        .find(|node| node.contains(&"10923-16383"))
        .unwrap();
    let source_port = node_port(source[1]);
    let destination_port = node_port(destination[1]);
    let slot = "9084";
    let migrating_key = "{one}:moving";
    let physical = connection.key(migrating_key);

    assert_eq!(
        cluster_cli(&container, &port, &["CLUSTER", "KEYSLOT", &physical]),
        slot
    );
    // This namespace's tag metadata and item keys share the migrating slot.
    let tagged_store = openrust::cache::Repository::new(Arc::new(RedisStore::new(
        connection.clone(),
        connection.clone(),
        "migration-8430:".into(),
    )));
    let tags = tagged_store.tags(&["artists", "daily"]).unwrap();
    let metadata = tags.getTags().tagKey("artists");
    assert_eq!(
        cluster_cli(&container, &port, &["CLUSTER", "KEYSLOT", &metadata]),
        slot
    );
    assert!(
        tags.forever("entry", &"before-migration".into())
            .await
            .unwrap()
    );
    assert_eq!(
        tags.get("entry").await.unwrap(),
        Some("before-migration".into())
    );

    assert_eq!(
        cluster_cli(
            &container,
            &destination_port,
            &["CLUSTER", "SETSLOT", slot, "IMPORTING", source[0]]
        ),
        "OK"
    );
    assert_eq!(
        cluster_cli(
            &container,
            &source_port,
            &["CLUSTER", "SETSLOT", slot, "MIGRATING", destination[0]]
        ),
        "OK"
    );
    assert!(cluster_cli(&container, &source_port, &["GET", &physical]).starts_with("ASK 9084 "));
    assert!(
        manager
            .connection(Some("no_redirects"))
            .unwrap()
            .set(migrating_key, b"must-not-arrive")
            .await
            .is_err()
    );
    connection.set(migrating_key, b"migrated").await.unwrap();
    assert_eq!(
        connection.get(migrating_key).await.unwrap().unwrap(),
        b"migrated"
    );

    let restricted = manager.connection(Some("restricted")).unwrap();
    let blocked_key = connection.key("{one}:acl");
    let error = restricted
        .command("HSET", &[blocked_key.as_bytes(), b"field", b"denied"])
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("Ask"), "{error:?}");
    assert_eq!(connection.get("{one}:acl").await.unwrap(), None);
    restricted.set("{one}:acl", b"single-key").await.unwrap();
    assert_eq!(
        connection.get("{one}:acl").await.unwrap().unwrap(),
        b"single-key"
    );

    let mut hash = connection.key("{one}:hash").into_bytes();
    hash.extend_from_slice(&[0, 0xff]);
    use openrust::redis::Value as RedisValue;
    assert_eq!(
        connection
            .command("hset", &[&hash, b"field", b"binary-key"])
            .await
            .unwrap(),
        RedisValue::Int(1)
    );
    assert_eq!(
        connection
            .command("HGET", &[&hash, b"field"])
            .await
            .unwrap(),
        RedisValue::BulkString(b"binary-key".to_vec())
    );

    let left = connection.key("{one}:raw-left");
    let right = connection.key("{one}:raw-right");
    // Redis allows the same-slot multi-key operation during import once all keys exist there.
    connection.set("{one}:raw-left", b"old").await.unwrap();
    connection.set("{one}:raw-right", b"old").await.unwrap();
    assert_eq!(
        connection
            .command("MSET", &[left.as_bytes(), b"1", right.as_bytes(), b"2"])
            .await
            .unwrap(),
        RedisValue::Okay
    );
    let stable_raw = connection.key("{two}:raw-stable");
    let moving_raw = connection.key("{one}:raw-mixed");
    let error = connection
        .command(
            "MSET",
            &[
                stable_raw.as_bytes(),
                b"stable",
                moving_raw.as_bytes(),
                b"moving",
            ],
        )
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("Ask"), "{error:?}");
    assert_eq!(
        connection.get("{two}:raw-stable").await.unwrap().unwrap(),
        b"stable"
    );
    assert_eq!(connection.get("{one}:raw-mixed").await.unwrap(), None);

    let migrate = |keys: &[&str]| {
        let mut args = vec![
            "MIGRATE",
            "127.0.0.1",
            &destination_port,
            "",
            "0",
            "5000",
            "AUTH",
            "cluster-fixture",
            "KEYS",
        ];
        args.extend_from_slice(keys);
        assert_eq!(cluster_cli(&container, &source_port, &args), "OK");
    };
    let tagged_item = tags.taggedItemKey("entry").await.unwrap();
    migrate(&[&tagged_item]);
    let error = tags
        .forever("entry", &"interrupted".into())
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("TryAgain"), "{error:?}");
    let remaining = cluster_cli(
        &container,
        &source_port,
        &["CLUSTER", "GETKEYSINSLOT", slot, "128"],
    );
    migrate(&remaining.lines().collect::<Vec<_>>());
    assert_eq!(
        tags.get("entry").await.unwrap(),
        Some("before-migration".into())
    );

    let cache = RedisStore::new(connection.clone(), connection.clone(), "cache:".into());
    let ttl = NonZeroU64::new(30).unwrap();
    let moving = "{one}:cache-moving";
    let stable = "{two}:cache-stable";
    assert!(cache.put(moving, &"moving".into(), ttl).await.unwrap());
    assert!(cache.put(stable, &"stable".into(), ttl).await.unwrap());
    assert_eq!(
        cache
            .many(&[moving, stable, "{one}:cache-missing", moving])
            .await
            .unwrap(),
        [
            Some("moving".into()),
            Some("stable".into()),
            None,
            Some("moving".into())
        ]
    );

    for node in [
        destination_port.as_str(),
        source_port.as_str(),
        port.as_str(),
    ] {
        assert_eq!(
            cluster_cli(
                &container,
                node,
                &["CLUSTER", "SETSLOT", slot, "NODE", destination[0]]
            ),
            "OK"
        );
    }
    assert!(cluster_cli(&container, &source_port, &["GET", &physical]).starts_with("MOVED 9084 "));
    assert_eq!(
        connection.get(migrating_key).await.unwrap().unwrap(),
        b"migrated"
    );

    assert_eq!(
        connection.get("{one}:raw-left").await.unwrap().unwrap(),
        b"1"
    );
    assert_eq!(
        tags.get("entry").await.unwrap(),
        Some("before-migration".into())
    );
    assert!(
        tagged_store
            .tags(&["artists"])
            .unwrap()
            .flush()
            .await
            .unwrap()
    );
    assert_eq!(tags.get("entry").await.unwrap(), None);

    let replica = nodes.iter().find(|node| node[3] == source[0]).unwrap();
    let replica_port = node_port(replica[1]);
    let surviving_key = "{c}:survives";
    let physical = connection.key(surviving_key);
    connection.set(surviving_key, b"replicated").await.unwrap();
    assert_eq!(
        cluster_cli(&container, &source_port, &["GET", &physical]),
        "replicated"
    );
    let replicated = tokio::time::Instant::now() + Duration::from_secs(8);
    while cluster_replica_read(&container, &replica_port, &physical) != "OK\nreplicated" {
        assert!(
            tokio::time::Instant::now() < replicated,
            "replica did not receive the write"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    cluster_cli(&container, &source_port, &["SHUTDOWN", "NOSAVE"]);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(18);
    while !cluster_cli(&container, &replica_port, &["ROLE"]).starts_with("master\n") {
        assert!(
            tokio::time::Instant::now() < deadline,
            "replica did not become primary"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    loop {
        match connection.get(surviving_key).await {
            Ok(Some(value)) => {
                assert_eq!(value, b"replicated");
                break;
            }
            result => assert!(
                tokio::time::Instant::now() < deadline,
                "cluster read did not recover: {result:?}"
            ),
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    connection.set(surviving_key, b"new-primary").await.unwrap();
    assert_eq!(
        cluster_cli(&container, &replica_port, &["GET", &physical]),
        "new-primary"
    );
}

#[tokio::test]
async fn redis_cluster_flushes_only_a_separate_lock_database() {
    use openrust::{
        cache::RedisStore,
        contracts::cache::{CanFlushLocks as _, Store as _},
    };
    use serde_json::json;

    if std::env::var_os("OPENRUST_TEST_REDIS_CLUSTER_PORT").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS_CLUSTER").is_none());
        return;
    }
    let seed = |name| {
        let port = std::env::var(name).unwrap().parse::<u16>().unwrap();
        json!({"host":"127.0.0.1","port":port,"password":"cluster-fixture"})
    };
    let second = seed("OPENRUST_TEST_REDIS_CLUSTER_SECOND_PORT");
    let manager = RedisManager::new(Arc::new(openrust::config::Repository::new(
        [(
            "database".into(),
            json!({"redis":{
                "direct_cluster_alias":second,
                "direct":seed("OPENRUST_TEST_REDIS_CLUSTER_STANDALONE_PORT"),
                "clusters":{
                    "data":[seed("OPENRUST_TEST_REDIS_CLUSTER_PORT")],
                    "alias":[second],
                    "independent":[seed("OPENRUST_TEST_REDIS_INDEPENDENT_CLUSTER_PORT")]
                },
            }}),
        )]
        .into(),
    )));
    let connection = |name| manager.connection(Some(name)).unwrap();
    let store =
        |data, locks| RedisStore::new(connection(data), connection(locks), "flush-fixture:".into());
    let sentinel = "{c}:cache";
    let ttl = NonZeroU64::new(30).unwrap();
    let data = store("data", "data");
    assert!(data.put(sentinel, &"keep".into(), ttl).await.unwrap());

    for (cache, locks) in [
        ("data", "alias"),
        ("data", "direct_cluster_alias"),
        ("direct_cluster_alias", "data"),
    ] {
        assert!(matches!(
            store(cache, locks).flushLocks().await,
            Err(CacheError::LockStoreNotSeparate)
        ));
        assert_eq!(data.get(sentinel).await.unwrap(), Some("keep".into()));
    }

    for (cache, locks) in [
        ("data", "independent"),
        ("data", "direct"),
        ("direct", "independent"),
    ] {
        let store = store(cache, locks);
        assert!(store.put(sentinel, &"keep".into(), ttl).await.unwrap());
        let mut left = store
            .lock("{one}:left", Some(Duration::from_secs(30)), None)
            .unwrap();
        let mut right = store
            .lock("{two}:right", Some(Duration::from_secs(30)), None)
            .unwrap();
        assert!(left.get().await.unwrap() && right.get().await.unwrap());
        assert!(store.flushLocks().await.unwrap());
        assert!(left.current_owner().await.unwrap().is_none());
        assert!(right.current_owner().await.unwrap().is_none());
        assert_eq!(store.get(sentinel).await.unwrap(), Some("keep".into()));
    }
}

#[tokio::test]
async fn redis_cluster_tags_flush_and_prune() {
    use openrust::{
        cache::{RedisStore, Repository},
        redis::Value as RedisValue,
    };
    use serde_json::json;

    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_CLUSTER_PORT") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS_CLUSTER").is_none());
        return;
    };
    let settings = Arc::new(openrust::config::Repository::new(
        [(
            "database".into(),
            json!({"redis":{"options":{"prefix":"{other}:"},"clusters":{
                "tags":[{"port":port.parse::<u16>().unwrap(),"password":"cluster-fixture"}]
            }}}),
        )]
        .into(),
    ));
    let connection = RedisManager::new(settings.clone())
        .connection(Some("tags"))
        .unwrap();
    let peer = RedisManager::new(settings)
        .connection(Some("tags"))
        .unwrap();
    let store = |connection: Arc<openrust::redis::connections::Connection>, prefix: &str| {
        Repository::new(Arc::new(RedisStore::new(
            connection.clone(),
            connection,
            prefix.into(),
        )))
    };
    let plain = store(connection.clone(), "tags*?:");
    let peer = store(peer, "tags*?:");
    let unrelated = store(connection.clone(), "unrelated:");
    let left = plain.tags(&["artists", "people"]).unwrap();
    let ordered = peer.tags(&["artists", "people"]).unwrap();
    let reversed = peer.tags(&["people", "artists", "artists"]).unwrap();
    let foreign = unrelated.tags(&["artists", "people"]).unwrap();
    assert!(plain.forever("item", &1.into()).await.unwrap());
    assert!(left.forever("item", &2.into()).await.unwrap());
    assert!(reversed.forever("item", &3.into()).await.unwrap());
    assert!(foreign.forever("item", &4.into()).await.unwrap());
    assert_eq!(ordered.get("item").await.unwrap(), Some(2.into()));
    assert_eq!(reversed.get("item").await.unwrap(), Some(3.into()));
    assert_eq!(left.get("missing").await.unwrap(), None);
    assert!(!left.add("item", &5.into(), None).await.unwrap());
    assert_eq!(left.increment("counter", i64::MAX).await.unwrap(), i64::MAX);
    assert_eq!(left.decrement("counter", 3).await.unwrap(), i64::MAX - 3);
    assert_eq!(
        ordered.pull("counter").await.unwrap(),
        Some((i64::MAX - 3).into())
    );
    assert_eq!(left.get("counter").await.unwrap(), None);

    let exists = async |key: &str| {
        connection
            .command("EXISTS", &[key.as_bytes()])
            .await
            .unwrap()
            == RedisValue::Int(1)
    };
    let index = async |name: &str| {
        let key = left.getTags().tagKey(name);
        let id = left.getTags().tagId(name).await.unwrap();
        format!("{}index:{id}", key.split_once("generation:").unwrap().0)
    };
    let count = async |key: &str| {
        connection
            .command("ZCARD", &[key.as_bytes()])
            .await
            .unwrap()
    };
    let registered = async |key: &str| {
        let (prefix, _) = key.rsplit_once("index:").unwrap();
        let registry = format!("{prefix}indexes");
        connection
            .command("SISMEMBER", &[registry.as_bytes(), key.as_bytes()])
            .await
            .unwrap()
            == RedisValue::Int(1)
    };
    let people = index("people").await;
    let removable = plain.tags(&["removable", "people"]).unwrap();
    let removable_index = index("removable").await;
    assert!(removable.forever("item", &9.into()).await.unwrap());
    assert!(registered(&removable_index).await);
    assert!(removable.forget("item").await.unwrap());
    assert!(!exists(&removable_index).await);
    assert!(!registered(&removable_index).await);
    assert!(registered(&people).await);
    assert!(removable.forever("item", &10.into()).await.unwrap());
    assert!(registered(&removable_index).await);
    assert_eq!(removable.pull("item").await.unwrap(), Some(10.into()));
    assert!(!registered(&removable_index).await);
    assert_eq!(count(&people).await, RedisValue::Int(2));

    assert!(
        left.put("short", &1.into(), Some(Ttl::Seconds(10)))
            .await
            .unwrap()
    );
    assert!(left.touch("short", Ttl::Seconds(1)).await.unwrap());
    assert_eq!(count(&people).await, RedisValue::Int(3));
    tokio::time::sleep(Duration::from_millis(1150)).await;
    assert_eq!(ordered.get("short").await.unwrap(), None);
    let artist = index("artists").await;
    let unknown = left.getTags().tagKey("unknown");
    assert!(!exists(&unknown).await);
    assert!(peer.tags(&[]).unwrap().flushStale().await.unwrap());
    assert_eq!(count(&artist).await, RedisValue::Int(3));
    let selected = peer.tags(&["artists", "unknown", "artists"]).unwrap();
    assert!(selected.flushStale().await.unwrap());
    assert_eq!(count(&artist).await, RedisValue::Int(2));
    assert_eq!(count(&people).await, RedisValue::Int(3));
    assert!(!exists(&unknown).await);
    assert_eq!(ordered.get("item").await.unwrap(), Some(2.into()));
    assert!(selected.flushStale().await.unwrap());
    plain.get_store().flush_stale_tags().await.unwrap();
    assert_eq!(count(&people).await, RedisValue::Int(2));

    let paged = plain.tags(&["paged"]).unwrap();
    let mut deleted = Vec::new();
    for number in 0..320 {
        let key = number.to_string();
        assert!(paged.forever(&key, &number.into()).await.unwrap());
        if number % 2 == 0 {
            deleted.push(paged.taggedItemKey(&key).await.unwrap());
        }
    }
    let keys = deleted.iter().map(String::as_bytes).collect::<Vec<_>>();
    assert_eq!(
        connection.command("DEL", &keys).await.unwrap(),
        RedisValue::Int(160)
    );
    let paged_index = index("paged").await;
    assert_eq!(count(&paged_index).await, RedisValue::Int(320));
    let foreign_key = foreign.taggedItemKey("item").await.unwrap();
    let invalid_bytes = b"\xffinvalid";
    let (prefix, _) = paged_index.rsplit_once("index:").unwrap();
    let registry = format!("{prefix}indexes");
    let foreign_tag = foreign.getTags().tagKey("artists");
    let foreign_id = foreign.getTags().tagId("artists").await.unwrap();
    let foreign_index = format!(
        "{}index:{foreign_id}",
        foreign_tag.rsplit_once("generation:").unwrap().0
    );
    let invalid_index = format!("{prefix}index:invalid");
    connection
        .command("SET", &[invalid_index.as_bytes(), b"unrelated"])
        .await
        .unwrap();
    connection
        .command(
            "SADD",
            &[
                registry.as_bytes(),
                foreign_index.as_bytes(),
                invalid_index.as_bytes(),
                invalid_bytes,
            ],
        )
        .await
        .unwrap();
    connection
        .command(
            "ZADD",
            &[
                paged_index.as_bytes(),
                b"-1",
                foreign_key.as_bytes(),
                b"-1",
                invalid_bytes,
            ],
        )
        .await
        .unwrap();
    assert_eq!(count(&paged_index).await, RedisValue::Int(322));
    plain.get_store().flush_stale_tags().await.unwrap();
    assert_eq!(count(&paged_index).await, RedisValue::Int(160));
    for member in [
        foreign_index.as_bytes(),
        invalid_index.as_bytes(),
        invalid_bytes,
    ] {
        assert_eq!(
            connection
                .command("SISMEMBER", &[registry.as_bytes(), member])
                .await
                .unwrap(),
            RedisValue::Int(0)
        );
    }
    assert!(registered(&paged_index).await);
    assert!(registered(&foreign_index).await);
    assert_eq!(count(&foreign_index).await, RedisValue::Int(1));
    assert!(exists(&invalid_index).await);
    assert!(exists(&foreign_key).await);
    assert_eq!(foreign.get("item").await.unwrap(), Some(4.into()));
    for number in 0..320 {
        let expected = (number % 2 != 0).then(|| number.into());
        assert_eq!(paged.get(&number.to_string()).await.unwrap(), expected);
    }

    let mut physical = vec![
        left.taggedItemKey("item").await.unwrap(),
        reversed.taggedItemKey("item").await.unwrap(),
    ];
    for number in 0..130 {
        let key = number.to_string();
        assert!(left.add(&key, &number.into(), None).await.unwrap());
        physical.push(left.taggedItemKey(&key).await.unwrap());
    }
    let artist = index("artists").await;
    let invalid = connection.key("tags*?:item");
    connection
        .command(
            "ZADD",
            &[
                artist.as_bytes(),
                b"-2",
                invalid.as_bytes(),
                b"-2",
                foreign_key.as_bytes(),
                b"-2",
                invalid_bytes,
            ],
        )
        .await
        .unwrap();
    assert!(plain.tags(&["artists"]).unwrap().flush().await.unwrap());
    for key in physical {
        assert!(!exists(&key).await);
    }
    plain.get_store().flush_stale_tags().await.unwrap();
    assert!(!exists(&people).await);
    assert!(!registered(&artist).await);
    assert!(exists(&invalid).await);
    assert_eq!(ordered.get("item").await.unwrap(), None);
    assert_eq!(plain.get("item").await.unwrap(), Some(1.into()));
    assert_eq!(foreign.get("item").await.unwrap(), Some(4.into()));

    let interrupted = plain.tags(&["interrupted"]).unwrap();
    assert!(interrupted.forever("retired", &7.into()).await.unwrap());
    let retired_value = interrupted.taggedItemKey("retired").await.unwrap();
    let retired_index = index("interrupted").await;
    let (prefix, suffix) = retired_index.rsplit_once("index:").unwrap();
    let (tag, _) = suffix.split_once(':').unwrap();
    let pending = format!("{prefix}pending:{tag}");
    let backup = format!("{retired_index}:backup");
    connection
        .command("RENAME", &[retired_index.as_bytes(), backup.as_bytes()])
        .await
        .unwrap();
    connection
        .command("SET", &[retired_index.as_bytes(), b"wrong type"])
        .await
        .unwrap();
    assert!(interrupted.flush().await.is_err());
    assert_eq!(interrupted.get("retired").await.unwrap(), None);
    connection
        .command("RENAME", &[backup.as_bytes(), retired_index.as_bytes()])
        .await
        .unwrap();
    connection
        .command(
            "ZADD",
            &[retired_index.as_bytes(), b"-2", invalid.as_bytes()],
        )
        .await
        .unwrap();
    assert!(interrupted.forever("current", &8.into()).await.unwrap());
    plain.get_store().flush_stale_tags().await.unwrap();
    assert!(exists(&retired_value).await);
    assert!(exists(&pending).await);
    assert_eq!(count(&retired_index).await, RedisValue::Int(1));
    connection
        .command("DEL", &[retired_value.as_bytes()])
        .await
        .unwrap();
    plain.get_store().flush_stale_tags().await.unwrap();
    assert!(!exists(&retired_index).await);
    assert!(!exists(&pending).await);
    assert_eq!(interrupted.get("current").await.unwrap(), Some(8.into()));

    let current_value = interrupted.taggedItemKey("current").await.unwrap();
    let current_index = index("interrupted").await;
    let (current_prefix, current_generation) = current_index.rsplit_once(':').unwrap();
    let empty_retirement = "0123456789abcdef0123456789abcdef";
    assert!(!exists(&format!("{current_prefix}:{empty_retirement}")).await);
    connection
        .command(
            "SADD",
            &[
                pending.as_bytes(),
                b"malformed",
                b"\xff",
                empty_retirement.as_bytes(),
            ],
        )
        .await
        .unwrap();
    assert!(interrupted.flush().await.unwrap());
    assert!(!exists(&pending).await);
    assert!(!exists(&current_value).await);
    assert!(!registered(&current_index).await);

    let empty_generation = interrupted.getTags().tagId("interrupted").await.unwrap();
    assert_ne!(
        empty_generation.rsplit_once(':').unwrap().1,
        current_generation
    );
    connection
        .command("SET", &[pending.as_bytes(), b"wrong type"])
        .await
        .unwrap();
    assert!(interrupted.flush().await.is_err());
    assert_eq!(
        interrupted.getTags().tagId("interrupted").await.unwrap(),
        empty_generation
    );
    connection
        .command("DEL", &[pending.as_bytes()])
        .await
        .unwrap();
    assert!(interrupted.forever("recovered", &9.into()).await.unwrap());
    assert!(interrupted.flush().await.unwrap());
    assert_eq!(interrupted.get("recovered").await.unwrap(), None);
}

#[tokio::test]
async fn redis_cluster_tls_follows_hostless_redirects() {
    use openrust::redis::Value as RedisValue;
    use serde_json::json;

    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_TLS_PORT") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS_TLS").is_none());
        return;
    };
    let container = std::env::var("OPENRUST_TEST_REDIS_TLS_CONTAINER").unwrap();
    let source = std::env::var("OPENRUST_TEST_REDIS_TLS_SOURCE").unwrap();
    let destination = std::env::var("OPENRUST_TEST_REDIS_TLS_DESTINATION").unwrap();
    let seed = format!("rediss://:cluster-fixture@127.0.0.1:{port}/0");
    let manager = RedisManager::new(Arc::new(openrust::config::Repository::new(
        [(
            "database".into(),
            json!({"redis":{
                "direct":{"url":seed,"max_retries":0},
                "clusters":{"tls":[{"url":"rediss://:cluster-fixture@127.0.0.2:1/0"},{"url":seed}]}
            }}),
        )]
        .into(),
    )));
    let connection = manager.connection(Some("tls")).unwrap();
    let initial = connection.get("{one}:tls").await;
    if std::env::var_os("OPENRUST_TEST_REDIS_TLS_UNTRUSTED").is_some() {
        assert!(initial.is_err(), "untrusted Redis certificate was accepted");
        let error = manager
            .connection(Some("direct"))
            .unwrap()
            .command("PING", &[])
            .await
            .unwrap_err();
        assert!(format!("{error:?}").contains("UnknownIssuer"), "{error:?}");
        return;
    }
    assert_eq!(initial.unwrap(), None);

    let cli = |port: &str, args: &[&str]| {
        let result = Command::new("docker")
            .args(["exec", "-e", "REDISCLI_AUTH=cluster-fixture", &container])
            .args([
                "redis-cli",
                "--raw",
                "--tls",
                "--cacert",
                "/tls/ca.pem",
                "-p",
                port,
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(result.status.success(), "{args:?}: {result:?}");
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    let slot = "9084";
    let source_id = cli(&source, &["CLUSTER", "MYID"]);
    let destination_id = cli(&destination, &["CLUSTER", "MYID"]);
    assert_eq!(cli(&source, &["CLUSTER", "KEYSLOT", "{one}:tls"]), slot);
    assert_eq!(
        cli(
            &source,
            &[
                "CONFIG",
                "SET",
                "cluster-preferred-endpoint-type",
                "unknown-endpoint"
            ]
        ),
        "OK"
    );
    assert_eq!(
        cli(
            &destination,
            &["CLUSTER", "SETSLOT", slot, "IMPORTING", &source_id]
        ),
        "OK"
    );
    assert_eq!(
        cli(
            &source,
            &["CLUSTER", "SETSLOT", slot, "MIGRATING", &destination_id]
        ),
        "OK"
    );
    assert_eq!(
        cli(&source, &["HGET", "{one}:tls", "field"]),
        format!("ASK {slot} :{destination}")
    );
    assert_eq!(
        connection
            .command("HSET", &[b"{one}:tls", b"field", b"secure"])
            .await
            .unwrap(),
        RedisValue::Int(1)
    );
    assert_eq!(
        connection
            .command("HGET", &[b"{one}:tls", b"field"])
            .await
            .unwrap(),
        RedisValue::BulkString(b"secure".to_vec())
    );
    for node in [&destination, &source, &port] {
        assert_eq!(
            cli(node, &["CLUSTER", "SETSLOT", slot, "NODE", &destination_id]),
            "OK"
        );
    }
    assert_eq!(
        cli(&source, &["HGET", "{one}:tls", "field"]),
        format!("MOVED {slot} :{destination}")
    );
    assert_eq!(
        connection
            .command("HGET", &[b"{one}:tls", b"field"])
            .await
            .unwrap(),
        RedisValue::BulkString(b"secure".to_vec())
    );
}
