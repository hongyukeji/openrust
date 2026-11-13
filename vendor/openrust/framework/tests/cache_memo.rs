use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::{CacheError, CacheManager, CacheResult, Ttl},
    contracts::{
        cache::{LockProvider as _, Repository as _},
        container::Container as _,
        foundation::Application as _,
        http::Kernel as _,
        queue::JobHandler,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{
        DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueManager, QueueResult,
    },
    routing::{Router, RoutingError},
    support::facades::Cache,
    testing::TestApp,
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const KEYS: &[&str] = &["counter", "missing", "binary"];

async fn exercise() -> CacheResult<i64> {
    let memo = Cache::memo(None)?;
    let same = Cache::memo(Some("redis"))?;
    let plain = Cache::store(None)?;
    assert!(Arc::ptr_eq(&memo, &same));
    let counter = memo.get("counter").await?.unwrap().as_i64().unwrap();
    assert!(memo.get("missing").await?.is_none());
    plain.increment("counter", 1).await?;
    plain.put("missing", &"out-of-band".into(), None).await?;
    assert_eq!(same.get("counter").await?, Some(counter.into()));
    assert_eq!(
        same.get_store()
            .many(&["missing", "binary", "counter", "binary"])
            .await?,
        [
            None,
            Some(vec![0, 255].into()),
            Some(counter.into()),
            Some(vec![0, 255].into())
        ],
    );
    assert_eq!(memo.increment("counter", 1).await?, counter + 2);
    assert_eq!(memo.get("counter").await?, Some((counter + 2).into()));
    assert!(!memo.add("missing", &"existing".into(), None).await?);
    assert_eq!(memo.get("missing").await?, Some("out-of-band".into()));
    assert!(memo.touch("missing", Ttl::Seconds(60)).await?);
    assert_eq!(memo.pull("missing").await?, Some("out-of-band".into()));
    assert!(memo.get("missing").await?.is_none());
    Ok(counter)
}

fn endpoint(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let value = exercise().await.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Response::json(StatusCode::OK, json!(value))
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/memo", endpoint)?;
    Ok(router)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(incoming).await
}

async fn request(address: SocketAddr) -> i64 {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!("GET /memo HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        response.split_once("\r\n\r\n").unwrap().1.parse().unwrap()
    })
    .await
    .unwrap()
}

struct MemoJob;

impl JobHandler for MemoJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let result = exercise().await.map_err(|source| QueueError::Execution {
                source: Box::new(source),
            })?;
            assert_eq!(payload.data()["initial"].as_i64(), Some(result));
            Ok(())
        })
    }
}

#[tokio::test]
async fn redis_memoized_cache_expires_between_http_requests_and_worker_jobs() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") else {
        return;
    };
    let mut fixture = TestApp::create(|builder| builder.withRouting(Routing::new(routes))).unwrap();
    let mut kernel = Kernel::new(
        Application::configure(fixture.root().to_owned())
            .withRouting(Routing::new(routes))
            .withConfig(|base, _| {
                let port = std::fs::read_to_string(base.join("redis-port")).unwrap();
                Ok([
                    ("cache".into(), json!({"default":"redis","stores":{"redis":{"driver":"redis"}}})),
                    ("database".into(), json!({"redis":{
                        "options":{"prefix":format!("openrust-memo:{}:",base.file_name().unwrap().to_string_lossy())},
                        "default":{"url":format!("redis://127.0.0.1:{port}/")}
                    }})),
                ].into())
            })
            .create().unwrap(),
    );
    std::fs::write(fixture.root().join("redis-port"), &port).unwrap();
    kernel.bootstrap().await.unwrap();
    let app = kernel.get_application();
    let config = app.config().unwrap();
    let cache = app.container().make::<CacheManager>().unwrap();
    let plain = cache.store(None).unwrap();
    plain.put("counter", &0.into(), None).await.unwrap();
    plain
        .put("binary", &vec![0, 255].into(), None)
        .await
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    assert_eq!(request(address).await, 0);
    assert_eq!(request(address).await, 2);
    server.abort();
    let _ = server.await;

    fixture.boot().await.unwrap();
    let worker_app = fixture.app();
    let worker_config = worker_app.config().unwrap();
    for key in ["cache", "database"] {
        worker_config.set(key, config.get(key).unwrap()).unwrap();
    }
    worker_config.set("queue.default", json!("redis")).unwrap();
    worker_config
        .set(
            "queue.connections.redis",
            json!({"driver":"redis","queue":"memo","retry_after":15}),
        )
        .unwrap();
    worker_config
        .set("queue.failed.driver", json!("null"))
        .unwrap();
    let queue = worker_app.container().make::<QueueManager>().unwrap();
    queue.register("cache.memo", Arc::new(MemoJob)).unwrap();
    for initial in [4, 6] {
        queue
            .dispatch(
                Payload::new("cache.memo", json!({"initial":initial})).unwrap(),
                DispatchOptions::default(),
                None,
            )
            .await
            .unwrap();
    }
    let result = fixture
        .artisan([
            "queue:work",
            "--stop-when-empty",
            "--sleep=0",
            "--timeout=0",
            "--tries=1",
        ])
        .await;
    assert_eq!(result.status, 0, "{}", result.stderr);
    assert_eq!(
        plain.get("counter").await.unwrap(),
        Some(8.into()),
        "{}",
        result.stdout
    );
    for key in KEYS {
        plain.forget(key).await.unwrap();
    }
}

#[tokio::test]
async fn memo_handle_cannot_serve_a_stale_snapshot_after_its_scope_closes() -> Result<(), CacheError>
{
    let mut fixture = TestApp::create(|builder| {
        builder.withConfig(|_, _| {
            Ok([(
                "cache".into(),
                json!({"default":"array","stores":{"array":{"driver":"array"}}}),
            )]
            .into())
        })
    })
    .unwrap();
    fixture.boot().await.unwrap();
    let container = fixture.app().container();
    let cache = container.make::<CacheManager>().unwrap();
    let plain = cache.store(None)?;
    let memo = container
        .scope(async {
            let memo = cache.memo(None)?;
            assert!(memo.get("key").await?.is_none());
            Ok::<_, CacheError>(memo)
        })
        .await?;
    plain.put("key", &"new".into(), None).await?;
    assert_eq!(memo.get("key").await?, Some("new".into()));
    plain.put("key", &"latest".into(), None).await?;
    assert_eq!(memo.get("key").await?, Some("latest".into()));
    let outside = cache.memo(None)?;
    assert_eq!(outside.get("key").await?, Some("latest".into()));
    assert_eq!(outside.get("miss").await?, None);
    outside.flush().await?;
    plain.put("miss", &"after flush".into(), None).await?;
    assert_eq!(outside.get("key").await?, None);
    assert_eq!(outside.get("miss").await?, Some("after flush".into()));
    let mut lock = outside.lock("memo-test", Some(Duration::from_secs(1)), None)?;
    assert!(lock.get().await?);
    assert!(lock.release().await?);
    Ok(())
}
