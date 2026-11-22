#![allow(non_snake_case, reason = "认证接口保留公开名称")]

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{Auth, AuthIdentifier, AuthManager},
    cache::rate_limiting::Limit,
    contracts::{
        auth::Authenticatable, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    foundation::{
        Application,
        configuration::{Middleware, Routing},
        http::Kernel,
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{MiddlewareEntry, RoutingError},
    support::{
        ProviderResult, ServiceProvider,
        facades::{RateLimiter, Route},
    },
};
use serde_json::json;

struct Identity;

impl Authenticatable for Identity {
    fn getAuthIdentifierName(&self) -> &str {
        "id"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        AuthIdentifier::from(7_u64)
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
}

struct Limits;

impl ServiceProvider for Limits {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<AuthManager>()?
                .viaRequest("header", |request| {
                    Box::pin(async move {
                        Ok(request
                            .headers()
                            .contains_key("x-user")
                            .then(|| Box::new(Identity) as Box<dyn Authenticatable>))
                    })
                });
            RateLimiter::forAsync("identity", |request| {
                Box::pin(async move {
                    let user = Auth::user(request)
                        .await?
                        .expect("限流前应完成路由指定的认证");
                    assert_eq!(user.getAuthIdentifier(), AuthIdentifier::from(7_u64));
                    Ok(Limit::perMinute(1).into())
                })
            })?;
            for (name, max) in [("api", 1), ("custom", 2)] {
                RateLimiter::r#for(name, move |request| {
                    Limit::perMinute(max).by(request.route("bucket").unwrap_or("missing"))
                })?;
            }
            RateLimiter::r#for("session", |request| {
                assert!(
                    request.hasSession(),
                    "限流策略应在有状态 API 的会话中间件之后执行"
                );
                Limit::perMinute(1)
            })?;
            Ok(())
        })
    }
}

fn empty(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn api() -> Result<(), RoutingError> {
    Route::bind::<String, _>("missing", |_| Box::pin(async { Ok(None) }))?;
    Route::get("/budget/{bucket}", empty)?;
    Route::getWith("/private", empty, |route| {
        route.middleware_name("auth:header");
        Ok(())
    })?;
    Route::get("/missing/{missing}", empty)?;
    Route::getWith("/excluded", empty, |route| {
        route.withoutMiddleware("throttle:api");
        Ok(())
    })
}

fn web() -> Result<(), RoutingError> {
    Route::get("/web", empty)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

struct Server {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    client: reqwest::Client,
}

impl Server {
    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("http://{}{path}", self.address))
            .header("accept", "application/json")
            .header("origin", "http://spa.example")
            .send()
            .await
            .unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start(configure: impl FnOnce(&mut Middleware) + Send + Sync + 'static) -> Server {
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            let mut config = std::collections::BTreeMap::from([
                ("cache".into(), json!({"default":"array","stores":{"array":{"driver":"array"}}})),
                ("app".into(), json!({"key":"a".repeat(32)})),
                ("session".into(), json!({"driver":"array","cookie":"api_test","lottery":[0,100]})),
                ("sanctum".into(), json!({"stateful":"spa.example"})),
                ("auth".into(), json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"},"header":{"driver":"header"}},"providers":{"users":{"driver":"database","table":"users"}}})),
            ]);
            if let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") {
                let prefix = std::env::var("OPENRUST_API_THROTTLE_PREFIX").expect("Redis 验收必须指定隔离前缀");
                config.insert("database".into(), json!({"redis":{"options":{"prefix":prefix},"default":{"url":format!("redis://127.0.0.1:{port}/")}}}));
            }
            Ok(config)
        })
        .withProviders(vec![Box::new(Limits)])
        .withMiddleware(configure)
        .withRouting(Routing::default().api(api).web(web))
        .create().unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport
                .with_state(Arc::new(kernel))
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Server {
        address,
        task,
        client: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    }
}

#[tokio::test]
async fn api_group_is_unthrottled_until_explicitly_enabled() {
    let server = start(|_| {}).await;
    for _ in 0..3 {
        let response = server.get("/api/budget/default").await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(!response.headers().contains_key("x-ratelimit-limit"));
    }
}

#[tokio::test]
async fn api_throttle_applies_before_bindings_and_supports_exclusions() {
    let server = start(|middleware| {
        middleware.throttleApi(Some("api"), false);
    })
    .await;
    let response = server.get("/api/budget/shared").await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(response.headers()["x-ratelimit-limit"], "1");
    assert_eq!(response.headers()["x-ratelimit-remaining"], "0");
    let denied = server.get("/api/budget/shared").await;
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(denied.headers().contains_key("retry-after"));
    assert!(denied.headers().contains_key("x-ratelimit-reset"));
    assert_eq!(
        server.get("/api/budget/other").await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        server.get("/api/missing/value").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        server.get("/api/missing/value").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    for path in ["/api/excluded", "/web"] {
        for _ in 0..3 {
            let response = server.get(path).await;
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert!(!response.headers().contains_key("x-ratelimit-limit"));
        }
    }
}

#[tokio::test]
async fn last_api_limiter_wins_and_false_values_disable_it() {
    let server = start(|middleware| {
        middleware
            .throttleApi(Some("api"), false)
            .throttleApi(Some("custom"), false);
    })
    .await;
    for _ in 0..2 {
        let response = server.get("/api/budget/custom").await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(response.headers()["x-ratelimit-limit"], "2");
    }
    assert_eq!(
        server.get("/api/budget/custom").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    for limiter in [None, Some(""), Some("0")] {
        let server = start(move |middleware| {
            middleware
                .throttleApi(Some("api"), false)
                .throttleApi(limiter, false);
        })
        .await;
        for _ in 0..3 {
            let response = server.get("/api/budget/disabled").await;
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert!(!response.headers().contains_key("x-ratelimit-limit"));
        }
    }
}

#[tokio::test]
async fn custom_api_group_overrides_default_throttling_in_either_call_order() {
    for group_first in [false, true] {
        let server = start(move |middleware| {
            if group_first {
                middleware.group("api", []);
            }
            middleware.throttleApi(Some("api"), false);
            if !group_first {
                middleware
                    .group("api", [MiddlewareEntry::named("custom-group")])
                    .group("custom-group", []);
            }
        })
        .await;
        for _ in 0..3 {
            assert_eq!(
                server.get("/api/budget/custom-group").await.status(),
                StatusCode::NO_CONTENT
            );
        }
        assert_eq!(
            server.get("/api/missing/value").await.status(),
            StatusCode::NO_CONTENT
        );
    }
}

#[tokio::test]
async fn api_throttle_does_not_implicitly_register_a_named_policy() {
    let server = start(|middleware| {
        middleware.throttleApi(Some("undefined-policy"), false);
    })
    .await;
    assert_eq!(
        server.get("/api/budget/unconfigured").await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn api_throttle_redis_option_shares_quota_between_applications() {
    if std::env::var_os("OPENRUST_TEST_REDIS_PORT").is_none() {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none(),
            "Redis 验收缺少端口配置"
        );
        return;
    }
    let configure = |middleware: &mut Middleware| {
        middleware
            .throttleApi(Some("api"), true)
            .throttleApi(Some("api"), false);
    };
    let first = start(configure).await;
    let second = start(configure).await;
    assert_eq!(
        first.get("/api/budget/redis").await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        second.get("/api/budget/redis").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn stateful_api_runs_before_throttling_in_either_call_order() {
    for stateful_first in [false, true] {
        let server = start(move |middleware| {
            if stateful_first {
                middleware.statefulApi();
            }
            middleware.throttleApi(Some("session"), false);
            if !stateful_first {
                middleware.statefulApi();
            }
        })
        .await;
        assert_eq!(
            server.get("/api/budget/session").await.status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            server.get("/api/budget/session").await.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}

#[tokio::test]
async fn authentication_runs_before_api_throttling() {
    let server = start(|middleware| {
        middleware.throttleApi(Some("identity"), false);
    })
    .await;
    assert_eq!(
        server.get("/api/private").await.status(),
        StatusCode::UNAUTHORIZED
    );
    for expected in [StatusCode::NO_CONTENT, StatusCode::TOO_MANY_REQUESTS] {
        let response = server
            .client
            .get(format!("http://{}/api/private", server.address))
            .header("accept", "application/json")
            .header("x-user", "7")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}
