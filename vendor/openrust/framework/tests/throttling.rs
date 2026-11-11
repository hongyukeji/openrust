use std::{
    io::{BufRead as _, BufReader},
    net::SocketAddr,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::rate_limiting::{Attempt, Limit},
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::{
        ProviderResult, ServiceProvider,
        facades::{RateLimiter, Route},
    },
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Limits;

impl ServiceProvider for Limits {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            RateLimiter::r#for("short", |_| Limit::perSecond(2))?;
            RateLimiter::r#for("shared", |_| Limit::perMinute(5))?;
            RateLimiter::r#for("multi", |request| {
                let mut limits = vec![Limit::perMinute(4).by("first")];
                if request.is("multi") {
                    limits.push(Limit::perMinute(1).by("strict"));
                }
                limits
            })?;
            RateLimiter::r#for("after", |_| {
                Limit::perMinute(1)
                    .after(|response| response.status() == StatusCode::BAD_REQUEST)
                    .response(|_, headers| {
                        Response::new(StatusCode::TOO_MANY_REQUESTS, "custom denial".into())
                            .withHeaders(headers)
                    })
            })?;
            RateLimiter::r#for("infinite", |_| Limit::none())?;
            Ok(())
        })
    }
}

fn empty(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn downstream(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let status = if request.route("result") == Some("failure") {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::NO_CONTENT
        };
        Ok(Response::empty(status))
    })
}

fn manual(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        fn convert(source: openrust::cache::CacheError) -> HttpError {
            HttpError::Internal {
                source: Box::new(source),
            }
        }
        let key = "manual-probe";
        RateLimiter::clear(key).await.map_err(convert)?;
        let Attempt::Executed(value) = RateLimiter::attempt(key, 1, async { Ok(7) }, 30)
            .await
            .map_err(convert)?
        else {
            panic!("expected callback to execute");
        };
        assert!(matches!(
            RateLimiter::attempt(key, 1, async { Ok(8) }, 30)
                .await
                .map_err(convert)?,
            Attempt::Throttled
        ));
        assert!(
            RateLimiter::tooManyAttempts(key, 1)
                .await
                .map_err(convert)?
        );
        assert!(RateLimiter::availableIn(key).await.map_err(convert)? > 0);
        let remaining = RateLimiter::remaining(key, 2).await.map_err(convert)?;
        RateLimiter::clear(key).await.map_err(convert)?;
        assert_eq!(RateLimiter::attempts(key).await.map_err(convert)?, 0);
        Response::json(StatusCode::OK, json!({"value":value,"remaining":remaining}))
    })
}

fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new(), |router| {
        router
            .get("/short", empty)?
            .middleware_name("throttle:short");
        router
            .get("/shared", empty)?
            .middleware_name("throttle:shared");
        router
            .get("/after/{result}", downstream)?
            .middleware_name("throttle:after");
        router
            .get("/unlimited", empty)?
            .middleware_name("throttle:infinite");
        router
            .get("/excluded", empty)?
            .middleware_name("throttle:short")
            .withoutMiddleware("throttle:short");
        router
            .get("/multi", empty)?
            .middleware_name("throttle:multi");
        router
            .get("/multi-probe", empty)?
            .middleware_name("throttle:multi");
        router.get("/manual", manual)?;
        Ok(())
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn start(redis: bool) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            let mut config = std::collections::BTreeMap::from([(
                "cache".into(),
                json!({"default":"array","stores":{"array":{"driver":"array"}}}),
            )]);
            if let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") {
                let prefix = std::env::var("OPENRUST_THROTTLE_FIXTURE").unwrap_or_default();
                let url = format!("redis://127.0.0.1:{port}/");
                config.insert(
                    "database".into(),
                    json!({"redis":{"options":{"prefix":prefix},"default":{"url":url}}}),
                );
            }
            Ok(config)
        })
        .withProviders(vec![Box::new(Limits)])
        .withMiddleware(move |middleware| {
            if redis {
                middleware.throttleWithRedis();
            }
        })
        .withRouting(Routing::default().web(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (address, server)
}

async fn send(address: SocketAddr, path: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut wire = String::new();
        stream.read_to_string(&mut wire).await.unwrap();
        wire
    }).await.unwrap()
}

fn status(response: &str) -> u16 {
    response
        .split_ascii_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn cache_throttling_returns_headers_retries_and_conditional_custom_responses() {
    let (address, server) = start(false).await;
    for remaining in [1, 0] {
        let result = send(address, "/short").await;
        assert_eq!(status(&result), 204);
        assert!(result.contains("x-ratelimit-limit: 2"));
        assert!(result.contains(&format!("x-ratelimit-remaining: {remaining}")));
    }
    let denied = send(address, "/short").await;
    assert_eq!(status(&denied), 429);
    assert!(denied.contains("retry-after: 1"));
    assert!(denied.contains("x-ratelimit-reset:"));
    assert!(denied.ends_with(r#"{"message":"Too Many Attempts."}"#));
    assert_eq!(status(&send(address, "/excluded").await), 204);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(status(&send(address, "/short").await), 204);
    assert_eq!(status(&send(address, "/after/success").await), 204);
    assert_eq!(status(&send(address, "/after/failure").await), 400);
    let denied = send(address, "/after/success").await;
    assert_eq!(status(&denied), 429);
    assert!(denied.ends_with("custom denial"));
    assert!(denied.contains("x-ratelimit-limit: 1"));
    let unlimited = send(address, "/unlimited").await;
    assert_eq!(status(&unlimited), 204);
    assert!(!unlimited.contains("x-ratelimit-limit"));
    let limited = send(address, "/multi").await;
    assert_eq!(status(&limited), 204);
    assert!(limited.contains("x-ratelimit-limit: 1"));
    assert_eq!(status(&send(address, "/multi").await), 429);
    let probe = send(address, "/multi-probe").await;
    assert_eq!(status(&probe), 204);
    assert!(probe.contains("x-ratelimit-remaining: 2"));
    let manual = send(address, "/manual").await;
    assert_eq!(status(&manual), 200);
    let (_, body) = manual.split_once("\r\n\r\n").unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(body).unwrap(),
        json!({"remaining":1,"value":7})
    );
    server.abort();
}

#[tokio::test]
async fn redis_fixture_process() {
    if std::env::var_os("OPENRUST_THROTTLE_FIXTURE").is_some() {
        let (address, server) = start(true).await;
        println!("THROTTLE LISTEN {address}");
        server.await.unwrap();
    }
}

struct Fixture(Child);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fixture(prefix: &str) -> (SocketAddr, Fixture) {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "redis_fixture_process", "--nocapture"])
        .env("OPENRUST_THROTTLE_FIXTURE", prefix)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut fixture = Fixture(child);
    let stdout = fixture.0.stdout.take().unwrap();
    let (ready, address) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(value) = line.strip_prefix("THROTTLE LISTEN ") {
                let _ = ready.send(value.parse::<SocketAddr>().unwrap());
                break;
            }
        }
    });
    (
        address.recv_timeout(Duration::from_secs(10)).unwrap(),
        fixture,
    )
}

#[tokio::test]
async fn redis_admission_is_atomic_across_two_http_processes() {
    if std::env::var_os("OPENRUST_TEST_REDIS_PORT").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none());
        return;
    }
    let prefix = format!("openrust-throttle-{:016x}:", fastrand::u64(..));
    let (first, _first) = fixture(&prefix);
    let (second, _second) = fixture(&prefix);
    let mut requests = tokio::task::JoinSet::new();
    for i in 0..24 {
        requests.spawn(send(if i % 2 == 0 { first } else { second }, "/shared"));
    }
    let mut allowed = 0;
    let mut denied = 0;
    while let Some(result) = requests.join_next().await {
        let response = result.unwrap();
        match status(&response) {
            204 => allowed += 1,
            429 => {
                denied += 1;
                assert!(response.contains("retry-after:"));
                assert!(response.contains("x-ratelimit-remaining: 0"));
            }
            _ => panic!("unexpected response {response}"),
        }
    }
    assert_eq!((allowed, denied), (5, 19));
}
