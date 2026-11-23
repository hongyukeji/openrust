#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{
    io::{BufRead as _, BufReader},
    net::SocketAddr,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{AuthIdentifier, AuthManager, AuthResult},
    cache::rate_limiting::{Attempt, Limit, LimiterResponse},
    contracts::{
        auth::Authenticatable, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    database::Value,
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
            RateLimiter::forAsync("body", |request| {
                Box::pin(async move {
                    let key = request
                        .input(Some("username"))
                        .await?
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    Ok(match key.as_str() {
                        "unlimited" => Limit::none().into(),
                        "response" => Response::new(StatusCode::ACCEPTED, "policy".into()).into(),
                        "error" => return Err(HttpError::InvalidRequest),
                        "multi" => {
                            vec![Limit::perMinute(2).by("same"), Limit::perHour(3).by("same")]
                                .into()
                        }
                        "after" => Limit::perMinute(1)
                            .by(key)
                            .after(|response| response.status() == StatusCode::NO_CONTENT)
                            .response(|_, headers| {
                                Response::new(StatusCode::TOO_MANY_REQUESTS, "async denial".into())
                                    .withHeaders(headers)
                            })
                            .into(),
                        _ => Limit::perMinute(1).by(key).into(),
                    })
                })
            })?;
            RateLimiter::r#for("9", |_| Limit::perMinute(1))?;
            RateLimiter::r#for("short", |_| Limit::perSecond(2))?;
            RateLimiter::r#for("shared", |_| Limit::perMinute(5))?;
            RateLimiter::r#for("multi-shared", |_| {
                vec![
                    Limit::perMinute(7).by("overall"),
                    Limit::perMinute(5).by("strict"),
                ]
            })?;
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

struct Identity(String);

impl Authenticatable for Identity {
    fn getAuthIdentifierName(&self) -> &str {
        "id"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        AuthIdentifier::String(self.0.clone())
    }
    fn getAuthPasswordName(&self) -> &str {
        "password"
    }
    fn getAuthPassword(&self) -> Option<&str> {
        None
    }
    fn getRememberToken(&self) -> Option<&str> {
        None
    }
    fn setRememberToken(&mut self, _: Option<String>) {}
    fn getRememberTokenName(&self) -> &str {
        "remember_token"
    }
    fn getAuthAttribute(&self, name: &str) -> AuthResult<Option<Value>> {
        Ok(match name {
            "upload_limit" => Some(Value::Text(" 3.9 ".into())),
            "job_limit" => Some(Value::Integer(2)),
            "invalid_limit" => Some(Value::Bool(true)),
            _ => None,
        })
    }
}

struct Authentication;

impl ServiceProvider for Authentication {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<AuthManager>()?
                .viaRequest("header", |request| {
                    Box::pin(async move {
                        Ok(request
                            .headers()
                            .get("x-fixture-user")
                            .and_then(|header| header.to_str().ok())
                            .map(|id| {
                                Box::new(Identity(format!("用户:{id}"))) as Box<dyn Authenticatable>
                            }))
                    })
                });
            Ok(())
        })
    }
}

fn empty(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn echo_body(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.input(Some("username")).await?;
        if input.as_ref().and_then(|v| v.as_str()) == Some("after") {
            return Ok(Response::empty(StatusCode::NO_CONTENT));
        }
        Response::json(StatusCode::OK, json!({"username":input}))
    })
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
        let key = "manual-probe";
        RateLimiter::clear(key).await?;
        let Attempt::Executed(value) = RateLimiter::attempt(key, 1, async { Ok(7) }, 30).await?
        else {
            panic!("expected callback to execute");
        };
        assert!(matches!(
            RateLimiter::attempt(key, 1, async { Ok(8) }, 30).await?,
            Attempt::Throttled
        ));
        assert!(RateLimiter::tooManyAttempts(key, 1).await?);
        assert!(RateLimiter::availableIn(key).await? > 0);
        let remaining = RateLimiter::remaining(key, 2).await?;
        RateLimiter::clear(key).await?;
        assert_eq!(RateLimiter::attempts(key).await?, 0);
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
            .get("/multi-shared", empty)?
            .middleware_name("throttle:multi-shared");
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
        router
            .post("/body", echo_body)?
            .middleware_name("throttle:body");
        for (path, throttle) in [
            ("/defaults", "throttle"),
            ("/budget", "throttle:1|2,1,budget"),
            ("/same-budget", "throttle:1|2,1,budget"),
            ("/other-budget", "throttle:1|2,1,other-budget"),
            ("/fraction", "throttle:1,0.02,fraction"),
            ("/numeric-name", "throttle:9"),
            ("/explicit-numeric", "throttle:9,1,explicit"),
            ("/model-budget", "throttle:1|upload_limit,1,model"),
            ("/model-direct", "throttle:job_limit,1,model-direct"),
            ("/model-invalid", "throttle:invalid_limit,1,model-invalid"),
            ("/model-missing", "throttle:missing_limit,1,model-missing"),
        ] {
            router.get(path, empty)?.middleware_name(throttle);
        }
        Ok(())
    })?;
    for domain in ["{tenant}.example.test", "other.test"] {
        Route::group(RouteGroup::new().domain(domain), |router| {
            router
                .get("/domain", empty)?
                .middleware_name("throttle:1,1,domain");
            Ok(())
        })?;
    }
    Ok(())
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn start(redis: bool) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .with_config(|_, _| {
            let mut config = std::collections::BTreeMap::from([
                (
                    "cache".into(),
                    json!({"default":"array","stores":{"array":{"driver":"array"}}}),
                ),
                (
                    "auth".into(),
                    json!({"defaults":{"guard":"header"},"guards":{"header":{"driver":"header"}}}),
                ),
            ]);
            if let Ok(port) = std::env::var("OPENRUST_THROTTLE_CLUSTER_PORT") {
                let prefix = std::env::var("OPENRUST_THROTTLE_FIXTURE").unwrap_or_default();
                config.insert(
                    "database".into(),
                    json!({"redis":{"options":{"prefix":prefix},"clusters":{
                        "default":[{"port":port.parse::<u16>().unwrap(),"password":"cluster-fixture"}]
                    }}}),
                );
            } else if let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") {
                let prefix = std::env::var("OPENRUST_THROTTLE_FIXTURE").unwrap_or_default();
                let url = format!("redis://127.0.0.1:{port}/");
                config.insert(
                    "database".into(),
                    json!({"redis":{"options":{"prefix":prefix},"default":{"url":url}}}),
                );
            }
            Ok(config)
        })
        .with_providers(vec![Box::new(Limits), Box::new(Authentication)])
        .with_middleware(move |middleware| {
            if redis {
                middleware.throttleWithRedis();
            }
        })
        .with_routing(Routing::default().web(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport
                .with_state(Arc::new(kernel))
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (address, server)
}

async fn send(address: SocketAddr, path: &str) -> String {
    send_as(address, path, "example.test", None).await
}

async fn send_as(address: SocketAddr, path: &str, host: &str, user: Option<&str>) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let identity = user.map_or_else(String::new, |id| format!("X-Fixture-User: {id}\r\n"));
        stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{identity}Accept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
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

fn fixture(prefix: &str, cluster_port: Option<&str>) -> (SocketAddr, Fixture) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "redis_fixture_process", "--nocapture"])
        .env("OPENRUST_THROTTLE_FIXTURE", prefix)
        .stdout(Stdio::piped());
    if let Some(port) = cluster_port {
        command.env("OPENRUST_THROTTLE_CLUSTER_PORT", port);
    }
    let child = command.spawn().unwrap();
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
    let (first, _first) = fixture(&prefix, None);
    let (second, _second) = fixture(&prefix, None);
    exercise_concurrent(first, second, "/shared").await;
}

async fn exercise_concurrent(first: SocketAddr, second: SocketAddr, path: &'static str) {
    let mut requests = tokio::task::JoinSet::new();
    for i in 0..24 {
        requests.spawn(send(if i % 2 == 0 { first } else { second }, path));
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

async fn exercise_numeric(first: SocketAddr, second: SocketAddr) {
    let default = send(first, "/defaults").await;
    assert_eq!(status(&default), 204, "{default}");
    assert!(default.contains("x-ratelimit-limit: 60"), "{default}");

    let guest = send(first, "/budget").await;
    assert_eq!(status(&guest), 204, "{guest}");
    assert!(guest.contains("x-ratelimit-limit: 1"), "{guest}");
    // A client-controlled Host change and a different route with the same prefix
    // both leave an unbound route's guest signature unchanged.
    assert_eq!(
        status(&send_as(second, "/same-budget", "different.test", None).await),
        429
    );
    assert_eq!(status(&send(first, "/other-budget").await), 204);

    let owner = send_as(first, "/budget", "example.test", Some("alpha")).await;
    assert_eq!(status(&owner), 204, "{owner}");
    assert!(owner.contains("x-ratelimit-limit: 2"), "{owner}");
    assert_eq!(
        status(&send_as(second, "/same-budget", "other.test", Some("alpha")).await),
        204
    );
    assert_eq!(
        status(&send_as(first, "/budget", "example.test", Some("alpha")).await),
        429
    );
    assert_eq!(
        status(&send_as(second, "/budget", "example.test", Some("beta")).await),
        204
    );

    assert_eq!(
        status(&send_as(first, "/domain", "a.example.test", None).await),
        204
    );
    assert_eq!(
        status(&send_as(second, "/domain", "b.example.test", None).await),
        429
    );
    assert_eq!(
        status(&send_as(second, "/domain", "other.test", None).await),
        204
    );

    assert_eq!(status(&send(first, "/fraction").await), 204);
    let fraction = send(second, "/fraction").await;
    assert_eq!(status(&fraction), 429, "{fraction}");
    assert!(fraction.contains("retry-after: 1"), "{fraction}");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(status(&send(second, "/fraction").await), 204);

    let named = send(first, "/numeric-name").await;
    assert!(named.contains("x-ratelimit-limit: 1"), "{named}");
    let numeric = send(second, "/explicit-numeric").await;
    assert!(numeric.contains("x-ratelimit-limit: 9"), "{numeric}");

    let guest = send(first, "/model-budget").await;
    assert!(guest.contains("x-ratelimit-limit: 1"), "{guest}");
    assert_eq!(status(&send(second, "/model-budget").await), 429);
    for (address, remaining) in [(first, 2), (second, 1), (first, 0)] {
        let allowed = send_as(address, "/model-budget", "example.test", Some("alpha")).await;
        assert_eq!(status(&allowed), 204, "{allowed}");
        assert!(allowed.contains("x-ratelimit-limit: 3"), "{allowed}");
        assert!(allowed.contains(&format!("x-ratelimit-remaining: {remaining}")));
    }
    let denied = send_as(second, "/model-budget", "example.test", Some("alpha")).await;
    assert_eq!(status(&denied), 429, "{denied}");
    let independent = send_as(second, "/model-budget", "example.test", Some("beta")).await;
    assert_eq!(status(&independent), 204, "{independent}");
    let direct = send_as(first, "/model-direct", "example.test", Some("alpha")).await;
    assert!(direct.contains("x-ratelimit-limit: 2"), "{direct}");
    assert_eq!(status(&send(first, "/model-direct").await), 500);
    for path in ["/model-invalid", "/model-missing"] {
        let invalid = send_as(first, path, "example.test", Some("alpha")).await;
        assert_eq!(status(&invalid), 500, "{invalid}");
    }
}

#[tokio::test]
async fn numeric_throttling_uses_matched_domains_and_authenticated_identifiers() {
    let (address, server) = start(false).await;
    exercise_numeric(address, address).await;
    server.abort();
}

#[tokio::test]
async fn numeric_throttling_shares_identity_buckets_across_redis_processes() {
    if std::env::var_os("OPENRUST_TEST_REDIS_PORT").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none());
        return;
    }
    let prefix = format!("openrust-throttle-{:016x}:", fastrand::u64(..));
    let (first, _first) = fixture(&prefix, None);
    let (second, _second) = fixture(&prefix, None);
    exercise_numeric(first, second).await;
}

#[tokio::test]
async fn redis_cluster_admits_all_named_quotas_atomically() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_CLUSTER_PORT") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS_CLUSTER").is_none());
        return;
    };
    let prefix = format!("{{}}:openrust-throttle-{:016x}:", fastrand::u64(..));
    let (first, _first) = fixture(&prefix, Some(&port));
    let (second, _second) = fixture(&prefix, Some(&port));
    let accepted = send(first, "/multi").await;
    assert_eq!(status(&accepted), 204, "{accepted}");
    assert!(accepted.contains("x-ratelimit-limit: 1"));
    let denied = send(second, "/multi").await;
    assert_eq!(status(&denied), 429, "{denied}");
    assert!(denied.contains("retry-after:"));
    let probe = send(second, "/multi-probe").await;
    assert_eq!(status(&probe), 204, "{probe}");
    assert!(probe.contains("x-ratelimit-remaining: 2"), "{probe}");
    assert_eq!(status(&send(first, "/after/success").await), 204);
    assert_eq!(status(&send(second, "/after/failure").await), 400);
    assert_eq!(status(&send(first, "/after/success").await), 429);
    exercise_concurrent(first, second, "/multi-shared").await;
    exercise_numeric(first, second).await;
}

async fn post_body(address: SocketAddr, body: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("POST /body HTTP/1.1\r\nHost: example.test\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut wire = String::new();
        stream.read_to_string(&mut wire).await.unwrap();
        wire
    }).await.unwrap()
}

#[tokio::test]
async fn asynchronous_named_policies_parse_body_preserve_errors_and_isolate_applications() {
    let (first, a) = start(false).await;
    let (second, b) = start(false).await;
    for name in ["alice", "bob"] {
        let body = json!({"username":name}).to_string();
        let accepted = post_body(first, &body).await;
        assert_eq!(status(&accepted), 200, "{accepted}");
        assert!(accepted.ends_with(&body));
        assert_eq!(status(&post_body(first, &body).await), 429);
        assert_eq!(status(&post_body(second, &body).await), 200);
    }
    for _ in 0..3 {
        let response = post_body(first, r#"{"username":"unlimited"}"#).await;
        assert_eq!(status(&response), 200);
        assert!(!response.contains("x-ratelimit-limit"));
        let response = post_body(first, r#"{"username":"response"}"#).await;
        assert_eq!(status(&response), 202);
        assert!(response.ends_with("policy"));
    }
    assert_eq!(
        status(&post_body(first, r#"{"username":"error"}"#).await),
        400
    );
    assert_eq!(status(&post_body(first, "{").await), 400);
    for remaining in [1, 0] {
        let response = post_body(first, r#"{"username":"multi"}"#).await;
        assert_eq!(status(&response), 200, "{response}");
        assert!(response.contains(&format!("x-ratelimit-remaining: {remaining}")));
    }
    assert_eq!(
        status(&post_body(first, r#"{"username":"multi"}"#).await),
        429
    );
    assert_eq!(
        status(&post_body(first, r#"{"username":"after"}"#).await),
        204
    );
    let denial = post_body(first, r#"{"username":"after"}"#).await;
    assert_eq!(status(&denial), 429);
    assert!(denial.ends_with("async denial"));
    a.abort();
    b.abort();
}

#[tokio::test]
async fn synchronous_and_asynchronous_policies_replace_each_other_atomically() {
    let limiter = openrust::cache::RateLimiter::default();
    limiter.r#for("replace", |_| Limit::perMinute(1));
    let original = limiter.limiter("replace").unwrap();
    assert!(limiter.asyncLimiter("replace").is_none());
    limiter.forAsync("replace", |_| Box::pin(async { Ok(Limit::none().into()) }));
    let asynchronous = limiter.asyncLimiter("replace").unwrap();
    assert!(limiter.limiter("replace").is_none());
    let (parts, body) = axum::http::Request::new(openrust::http::Body::empty()).into_parts();
    let mut request = Request::from_parts(parts, body);
    let LimiterResponse::Limit(limit) = original(&request) else {
        panic!("original policy changed")
    };
    assert_eq!(limit.maxAttempts, 1);
    limiter.r#for("replace", |_| Limit::perMinute(2));
    assert!(limiter.limiter("replace").is_some());
    assert!(limiter.asyncLimiter("replace").is_none());
    assert!(matches!(
        asynchronous(&mut request).await.unwrap(),
        LimiterResponse::Unlimited(_)
    ));
    let LimiterResponse::Limit(limit) = limiter.limiter("replace").unwrap()(&request) else {
        panic!("replacement missing")
    };
    assert_eq!(limit.maxAttempts, 2);
}

#[tokio::test]
async fn asynchronous_named_policies_share_redis_buckets_across_http_processes() {
    if std::env::var_os("OPENRUST_TEST_REDIS_PORT").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none());
        return;
    }
    let prefix = format!("openrust-async-throttle-{:016x}:", fastrand::u64(..));
    let (first, _first) = fixture(&prefix, None);
    let (second, _second) = fixture(&prefix, None);
    for name in ["alice", "bob"] {
        let body = json!({"username":name}).to_string();
        let response = post_body(first, &body).await;
        assert_eq!(status(&response), 200, "{response}");
        let denied = post_body(second, &body).await;
        assert_eq!(status(&denied), 429, "{denied}");
        assert!(denied.contains("x-ratelimit-remaining: 0"));
    }
}
