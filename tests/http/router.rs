//! 通过 `tower::ServiceExt::oneshot` 验证 HTTP 层的端到端行为。

use axum::body::{Body, to_bytes};
use axum::extract::{Extension, Request};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt as _;
use http_body::Body as _;
use openrust::ExecutionContext;
use openrust::config::FrameworkConfig;
use openrust::core::container::Scope;
use openrust::core::{RateLimitBucket, RateLimitFuture, RateLimitStore, RateLimiter};
use openrust::http::RateLimitLayer;
use openrust::http::axum;
use openrust::http::{
    Created, HttpServiceProvider, Inject, NoContent, PageParams, Paginated, RouteGroup,
    RouteUrlGenerator, Validated, build_router,
};
use openrust::http::{Json, Path, Query};

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use validator::Validate;

type TestResult = Result<(), Box<dyn std::error::Error>>;
static REQUEST_CANCELLED: AtomicBool = AtomicBool::new(false);

struct ScopeProbe(Arc<Mutex<Option<Arc<Scope>>>>);

async fn streamed_scoped_identity(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&scope));
    Body::from_stream(
        futures_util::stream::once(async move {
            let identity = scope.resolve::<RequestIdentity>().await;
            Ok::<_, std::io::Error>(bytes::Bytes::from(match identity {
                Ok(identity) => identity.0.to_string(),
                Err(error) => error.code().to_owned(),
            }))
        })
        .chain(futures_util::stream::iter([Ok(bytes::Bytes::from_static(
            b"next",
        ))])),
    )
    .into_response()
}

#[tokio::test]
async fn scoped_factory_survives_response_into_body_until_stream_finishes() -> TestResult {
    let probe = Arc::new(Mutex::new(None));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(ScopeProbe(Arc::clone(
            &probe,
        )))))
        .provider(IdentityProvider::new(|_| {
            Box::pin(async { Ok(Arc::new(RequestIdentity(7))) })
        }))
        .provider(TestRoutes::new(|routes| {
            routes.get("/streamed-scope", streamed_scoped_identity)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/streamed-scope")
                .body(Body::empty())?,
        )
        .await?;
    let mut body = response.into_body();
    let scope = probe
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or("missing scope")?;
    assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
    let first = futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
        .await
        .ok_or("missing first frame")??;
    assert_eq!(
        first.into_data().map_err(|_| "expected data")?.as_ref(),
        b"7"
    );
    assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
    let second = futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
        .await
        .ok_or("missing second frame")??;
    assert_eq!(
        second.into_data().map_err(|_| "expected data")?.as_ref(),
        b"next"
    );
    assert!(
        futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
            .await
            .is_none()
    );
    assert_eq!(
        scope
            .resolve::<RequestIdentity>()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("scope_closed".to_owned())
    );
    Ok(())
}

async fn scoped_empty(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    Response::new(Body::empty())
}

async fn scoped_error(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    Response::new(Body::from_stream(futures_util::stream::once(async {
        Err::<bytes::Bytes, _>(std::io::Error::other("stream failed"))
    })))
}

async fn scoped_pending_body(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    Response::new(Body::from_stream(futures_util::stream::pending::<
        std::result::Result<bytes::Bytes, std::io::Error>,
    >()))
}

struct CancelScopeProbe {
    scope: Arc<Mutex<Option<Arc<Scope>>>>,
    ready: Arc<tokio::sync::Notify>,
}

async fn scoped_pending_handler(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<CancelScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .scope
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    probe.ready.notify_one();
    std::future::pending::<Response>().await
}

async fn scope_test_router(
    probe: Arc<Mutex<Option<Arc<Scope>>>>,
    ready: Arc<tokio::sync::Notify>,
) -> openrust::Result<axum::Router> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(ScopeProbe(Arc::clone(
            &probe,
        )))))
        .provider(TestInstance(std::sync::Arc::new(CancelScopeProbe {
            scope: probe,
            ready,
        })))
        .provider(IdentityProvider::new(|_| {
            Box::pin(async { Ok(Arc::new(RequestIdentity(7))) })
        }))
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/empty", scoped_empty)
                .get("/error", scoped_error)
                .get("/pending-body", scoped_pending_body)
                .get("/pending-handler", scoped_pending_handler)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    build_router(&app)
}

fn observed_scope(probe: &Mutex<Option<Arc<Scope>>>) -> TestResultScope {
    probe
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or_else(|| "missing scope".into())
}

type TestResultScope = Result<Arc<Scope>, Box<dyn std::error::Error>>;

#[tokio::test]
async fn scope_closes_when_response_body_dropped_or_empty() -> TestResult {
    for path in ["/pending-body", "/empty"] {
        let probe = Arc::new(Mutex::new(None));
        let router =
            scope_test_router(Arc::clone(&probe), Arc::new(tokio::sync::Notify::new())).await?;
        let response = router
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        let body = response.into_body();
        let scope = observed_scope(&probe)?;
        assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
        drop(body);
        assert_eq!(
            scope
                .resolve::<RequestIdentity>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("scope_closed".to_owned())
        );
    }
    Ok(())
}

#[tokio::test]
async fn scope_closes_on_body_error_before_body_drop() -> TestResult {
    let probe = Arc::new(Mutex::new(None));
    let router =
        scope_test_router(Arc::clone(&probe), Arc::new(tokio::sync::Notify::new())).await?;
    let response = router
        .oneshot(Request::builder().uri("/error").body(Body::empty())?)
        .await?;
    let mut body = response.into_body();
    let scope = observed_scope(&probe)?;
    assert!(
        futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
            .await
            .ok_or("missing error frame")?
            .is_err()
    );
    assert_eq!(
        scope
            .resolve::<RequestIdentity>()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("scope_closed".to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_handler_closes_scope_even_if_scope_is_retained() -> TestResult {
    let probe = Arc::new(Mutex::new(None));
    let ready = Arc::new(tokio::sync::Notify::new());
    let router = scope_test_router(Arc::clone(&probe), Arc::clone(&ready)).await?;
    let request = Request::builder()
        .uri("/pending-handler")
        .body(Body::empty())?;
    let request = tokio::spawn(async move { router.oneshot(request).await });
    ready.notified().await;
    let scope = observed_scope(&probe)?;
    assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
    request.abort();
    assert!(request.await.is_err());
    assert_eq!(
        scope
            .resolve::<RequestIdentity>()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("scope_closed".to_owned())
    );
    Ok(())
}

#[derive(Debug)]
struct RequestIdentity(usize);

async fn scoped_identity(
    Inject(first): Inject<RequestIdentity>,
    Inject(second): Inject<RequestIdentity>,
) -> String {
    format!("{}:{}", first.0, second.0)
}

#[tokio::test]
async fn inject_resolves_async_scoped_factory_once_per_request() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(HttpServiceProvider);
    builder.provider(IdentityProvider::new({
        let calls = Arc::clone(&calls);
        move |_| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                tokio::task::yield_now().await;
                Ok(Arc::new(RequestIdentity(
                    calls.fetch_add(1, Ordering::SeqCst) + 1,
                )))
            })
        }
    }));
    builder.provider(TestRoutes::new(|routes| {
        routes.get("/scoped-identity", scoped_identity)
    }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for expected in ["1:1", "2:2"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/scoped-identity")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await?;
        assert_eq!(body.as_ref(), expected.as_bytes());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

fn is_utc_rfc3339(value: &serde_json::Value) -> bool {
    let Some(value) = value.as_str() else {
        return false;
    };
    let bytes = value.as_bytes();
    value.ends_with('Z')
        && bytes.get(19) == Some(&b'.')
        && bytes
            .get(20..23)
            .is_some_and(|millis| millis.iter().all(u8::is_ascii_digit))
        && chrono::DateTime::parse_from_rfc3339(value).is_ok()
}

struct Greeting(&'static str);

#[derive(Deserialize, Validate)]
struct CreatePost {
    #[validate(length(min = 3, message = "title must be at least 3 characters"))]
    title: String,
    #[validate(range(min = 1))]
    pages: u32,
}

async fn greet(Inject(greeting): Inject<Greeting>) -> Json<&'static str> {
    Json(greeting.0)
}

async fn create(Validated(Json(input)): Validated<Json<CreatePost>>) -> Json<String> {
    Json(input.title)
}

async fn missing(Inject(_greeting): Inject<String>) -> &'static str {
    "unreachable"
}

async fn global_layer_value(Extension(value): Extension<&'static str>) -> &'static str {
    value
}

async fn add_shared_extension(mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    request.extensions_mut().insert("shared");
    let response = next.run(request).await;
    if path == "/plain-error" {
        (StatusCode::INTERNAL_SERVER_ERROR, "private detail").into_response()
    } else {
        response
    }
}

// 此处的 panic 是被测行为：它必须转换为 500 envelope，不能导致服务器退出。
#[allow(clippy::panic)]
async fn boom() -> &'static str {
    panic!("handler exploded")
}

async fn router() -> openrust::Result<axum::Router> {
    router_with_config(FrameworkConfig::default()).await
}

async fn router_with_config(config: FrameworkConfig) -> openrust::Result<axum::Router> {
    let mut config = config;
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    router_with_exact_config(config).await
}

async fn router_with_exact_config(config: FrameworkConfig) -> openrust::Result<axum::Router> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(Greeting("hello"))))
        .provider(TestRoutes::new(|r| {
            r.nest("/api", |api| {
                api.get("/greet", greet)
                    .post("/posts", create)
                    .get("/missing", missing)
                    .get("/boom", boom)
                    .get("/large", large)
                    .get("/slow", slow)
                    .get("/execution", execution_context_info)
                    .get("/cancel", cancellation_probe)
                    .post("/raw", create_raw)
                    .get("/items/{id}", item)
                    .get("/search", search)
                    .get("/own-404", own_not_found)
                    .post("/created", created)
                    .delete("/no-content", no_content)
                    .get("/page", page)
                    .get("/limited", limited)
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    build_router(&app)
}

#[derive(Deserialize, serde::Serialize)]
struct RawPost {
    title: String,
    pages: u32,
}

async fn create_raw(Json(input): Json<RawPost>) -> Json<RawPost> {
    Json(input)
}
async fn item(Path(id): Path<u64>) -> Json<u64> {
    Json(id)
}
async fn search(Query(id): Query<RawPost>) -> Json<String> {
    Json(id.title)
}
async fn own_not_found() -> openrust::Error {
    openrust::Error::NotFound("my resource is absent".into())
}

async fn grouped_route() -> &'static str {
    "grouped"
}

async fn generated_named_url(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    let parameters = std::collections::BTreeMap::from([
        ("id".to_owned(), "a b/c".to_owned()),
        ("page".to_owned(), "2".to_owned()),
        ("filter".to_owned(), "tea & cake".to_owned()),
    ]);
    urls.route("admin.users.show", &parameters).map(Json)
}

async fn generated_named_url_missing_parameter(
    urls: RouteUrlGenerator,
) -> openrust::Result<Json<String>> {
    urls.route("admin.users.show", &std::collections::BTreeMap::new())
        .map(Json)
}

async fn generated_outside_group_name(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    urls.route(
        "admin.management.outside",
        &std::collections::BTreeMap::new(),
    )
    .map(Json)
}

async fn generated_trailing_slash_url(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    urls.route("posts.trailing", &std::collections::BTreeMap::new())
        .map(Json)
}

async fn grouped_order(Extension(order): Extension<Vec<&'static str>>) -> Json<Vec<&'static str>> {
    Json(order)
}

async fn grouped_outside() -> Json<Vec<&'static str>> {
    Json(Vec::new())
}

async fn mark_route_group(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-route-group", axum::http::HeaderValue::from_static("api"));
    response
}

async fn mark_first_group_middleware(mut request: Request, next: Next) -> Response {
    request.extensions_mut().insert(true);
    next.run(request).await
}

async fn observe_first_group_middleware(request: Request, next: Next) -> Response {
    let first_middleware_ran = request.extensions().get::<bool>().copied().unwrap_or(false);
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-middleware-order",
        axum::http::HeaderValue::from_static(if first_middleware_ran {
            "declared"
        } else {
            "reversed"
        }),
    );
    response
}

async fn record_group_middleware(
    axum::extract::State(label): axum::extract::State<&'static str>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(order) = request.extensions_mut().get_mut::<Vec<&'static str>>() {
        order.push(label);
    } else {
        request.extensions_mut().insert(vec![label]);
    }
    next.run(request).await
}

#[tokio::test]
async fn nested_route_groups_combine_prefixes_and_scope_middleware() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(
                    RouteGroup::new()
                        .prefix("api/")
                        .middleware(axum::middleware::from_fn(mark_route_group))
                        .middleware(axum::middleware::from_fn(mark_first_group_middleware))
                        .middleware(axum::middleware::from_fn(observe_first_group_middleware))
                        .middleware(axum::middleware::from_fn_with_state(
                            "outer-first",
                            record_group_middleware,
                        ))
                        .middleware(axum::middleware::from_fn_with_state(
                            "outer-second",
                            record_group_middleware,
                        )),
                    |api| {
                        api.group(
                            RouteGroup::new().prefix("/v1/").middleware(
                                axum::middleware::from_fn_with_state(
                                    "inner",
                                    record_group_middleware,
                                ),
                            ),
                            |v1| v1.get("/grouped", grouped_order),
                        )
                    },
                )
                .get("/outside-group", grouped_route)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;

    let response = build_router(&app)?
        .oneshot(
            Request::builder()
                .uri("/api/v1/grouped")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-route-group"], "api");
    assert_eq!(response.headers()["x-middleware-order"], "declared");
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["outer-first", "outer-second", "inner"])
    );

    let response = build_router(&app)?
        .oneshot(
            Request::builder()
                .uri("/outside-group")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("x-route-group"));
    Ok(())
}

struct NamedMiddlewareProvider;

impl openrust::core::ServiceProvider for NamedMiddlewareProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_alias(
                "first",
                axum::middleware::from_fn_with_state("first", record_group_middleware),
            )?;
            app.middleware_alias(
                "second",
                axum::middleware::from_fn_with_state("second", record_group_middleware),
            )?;
            app.middleware_group("composed", ["first", "second"])?;
            app.middleware_group("nested", ["composed", "first"])?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn named_middleware_aliases_and_nested_groups_keep_declaration_order() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(NamedMiddlewareProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/route-scope/private", grouped_order)
                .middleware_alias("second")
                .get("/route-scope/public", grouped_outside)
                .group(
                    RouteGroup::new()
                        .prefix("/private")
                        .middleware_group("nested"),
                    |private| private.get("/named", grouped_order),
                )
                .group(
                    RouteGroup::new()
                        .prefix("/alias")
                        .middleware_alias("second"),
                    |private| private.get("/named", grouped_order),
                )
                .get("/outside", grouped_outside)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/private/named")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["first", "second", "first"])
    );

    let response = router
        .clone()
        .oneshot(Request::builder().uri("/alias/named").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["second"])
    );

    let response = router
        .clone()
        .oneshot(Request::builder().uri("/outside").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], serde_json::json!([]));

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/route-scope/private")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["second"])
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/route-scope/public")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], serde_json::json!([]));
    Ok(())
}

struct DuplicateMiddlewareProvider;

impl openrust::core::ServiceProvider for DuplicateMiddlewareProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_alias("shared", axum::middleware::from_fn(mark_route_group))?;
            app.middleware_group("shared", ["shared"])?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn duplicate_middleware_names_fail_provider_boot() -> TestResult {
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(DuplicateMiddlewareProvider);
    let error = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(FrameworkConfig::default()),
        ))
        .await
        .err()
        .ok_or("duplicate middleware name unexpectedly started")?;
    assert_eq!(error.code(), "http_middleware_name_duplicate");
    Ok(())
}

#[tokio::test]
async fn unknown_or_cyclic_named_middleware_fails_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().middleware_alias("missing"), |group| {
                group.get("/unknown-middleware", grouped_order)
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("unknown named middleware unexpectedly built")?;
    assert_eq!(error.code(), "http_middleware_name_unknown");

    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(CyclicMiddlewareProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().middleware_group("first"), |group| {
                group.get("/cyclic-middleware", grouped_order)
            })
        }));
    let app = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(FrameworkConfig::default()),
        ))
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("cyclic named middleware unexpectedly built")?;
    assert_eq!(error.code(), "http_middleware_group_cycle");
    Ok(())
}

#[test]
fn named_middleware_without_registry_fails_closed_on_all_route_paths() -> TestResult {
    let direct = openrust::http::Routes::new()
        .get("/private", grouped_order)
        .middleware_alias("auth")
        .get("/public", grouped_outside)
        .into_router()
        .err()
        .ok_or("named middleware without an application registry was silently ignored")?;
    assert_eq!(direct.code(), "http_middleware_registry_unavailable");

    let nested = openrust::http::Routes::new()
        .group(RouteGroup::new(), |routes| {
            routes
                .get("/private", grouped_order)
                .middleware_group("auth")
        })
        .into_router()
        .err()
        .ok_or("nested named middleware without a registry was silently ignored")?;
    assert_eq!(nested.code(), "http_middleware_registry_unavailable");

    let group = openrust::http::Routes::new()
        .group(RouteGroup::new().middleware_alias("auth"), |routes| {
            routes.get("/private", grouped_order)
        })
        .into_router()
        .err()
        .ok_or("group middleware without a registry was silently ignored")?;
    assert_eq!(group.code(), "http_middleware_registry_unavailable");
    Ok(())
}

struct CyclicMiddlewareProvider;

impl openrust::core::ServiceProvider for CyclicMiddlewareProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_group("first", ["second"])?;
            app.middleware_group("second", ["first"])?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn duplicate_routes_across_groups_fail_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(RouteGroup::new().prefix("api"), |api| {
                    api.get("/duplicate", grouped_route)
                })
                .group(RouteGroup::new().prefix("/api/"), |api| {
                    api.get("/duplicate", grouped_route)
                })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("duplicate route did not fail during router build")?;
    assert_eq!(error.code(), "http_route_registration_failed");
    Ok(())
}

#[tokio::test]
async fn named_route_urls_use_group_prefix_parameters_and_configured_host() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        serde_json::json!({
            "app": {"url": "https://app.example.test/root"},
            "api": {"http_status": "semantic"}
        }),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(
                    RouteGroup::new().prefix("api/").name_prefix("admin."),
                    |api| {
                        api.group(RouteGroup::new().name_prefix("users."), |users| {
                            users.get("/users/{id}", generated_named_url).name("show")
                        })
                        .get("/missing", generated_named_url_missing_parameter)
                    },
                )
                .group(
                    RouteGroup::new().prefix("/root").name_prefix("admin."),
                    |root| {
                        root.group(
                            RouteGroup::new().prefix("/api").name_prefix("management."),
                            |api| api.get("/outside-name", generated_outside_group_name),
                        )
                    },
                )
                .name("outside")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/users/1")
                .header(header::HOST, "attacker.example")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        "https://app.example.test/root/api/users/a%20b%2Fc?filter=tea+%26+cake&page=2"
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/root/api/outside-name")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        "https://app.example.test/root/root/api/outside-name"
    );

    let response = router
        .oneshot(Request::builder().uri("/api/missing").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["code"],
        "http_route_parameter_missing"
    );
    Ok(())
}

#[tokio::test]
async fn named_url_preserves_trailing_slash_and_targets_a_real_route() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/generate-trailing", generated_trailing_slash_url)
                .get("/api/posts/", grouped_route)
                .name("posts.trailing")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/generate-trailing")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let generated = json_body(response).await["data"]
        .as_str()
        .ok_or("generated URL was not a string")?
        .to_owned();
    let path = ::url::Url::parse(&generated)?.path().to_owned();
    assert_eq!(path, "/api/posts/");
    let response = router
        .oneshot(Request::builder().uri(path).body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn duplicate_named_routes_fail_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/first", grouped_route)
                .name("shared")
                .get("/second", grouped_route)
                .name("shared")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("duplicate route names unexpectedly built")?;
    assert_eq!(error.code(), "http_route_name_duplicate");
    Ok(())
}

#[tokio::test]
async fn named_wildcard_routes_fail_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/files/{*rest}", grouped_route)
                .name("files.show")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("named wildcard route unexpectedly built")?;
    assert_eq!(error.code(), "http_route_pattern_unsupported");
    Ok(())
}

#[tokio::test]
async fn named_routes_with_empty_segments_fail_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().prefix("/api"), |api| {
                api.get("/files//", grouped_route).name("files.latest")
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("named route with empty path segment unexpectedly built")?;
    assert_eq!(error.code(), "http_route_pattern_unsupported");
    Ok(())
}

#[tokio::test]
async fn named_group_root_with_repeated_slashes_fails_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().prefix("/api"), |api| {
                api.get("//", grouped_route).name("api.double-root")
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("named route with repeated root slashes unexpectedly built")?;
    assert_eq!(error.code(), "http_route_pattern_unsupported");
    Ok(())
}

async fn created() -> openrust::Result<openrust::http::Located<Json<u64>>> {
    Created::with_location("/api/items/42", Json(42))
}

async fn no_content() -> NoContent {
    NoContent
}

async fn page() -> Paginated<u64> {
    Paginated::new(vec![1, 2], PageParams::default(), 57)
}

async fn rate_limited_ok() -> &'static str {
    "ok"
}

async fn limited() -> openrust::Error {
    openrust::Error::rate_limited(30)
}

async fn large() -> Json<String> {
    Json("compressible response ".repeat(100))
}

async fn slow() -> Json<&'static str> {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Json("late")
}

async fn execution_context_info() -> Json<serde_json::Value> {
    let current = ExecutionContext::current();
    Json(serde_json::json!({
        "request_id": current.as_ref().and_then(|context| context.metadata().request_id()),
        "has_budget": current.as_ref().and_then(ExecutionContext::remaining_budget).is_some(),
    }))
}

async fn cancellation_probe() -> &'static str {
    if let Some(context) = ExecutionContext::current() {
        tokio::spawn(async move {
            context.cancelled().await;
            REQUEST_CANCELLED.store(true, Ordering::SeqCst);
        });
    }
    std::future::pending::<&'static str>().await
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tokio::test]
async fn health_and_request_id_are_installed_by_the_plugin() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/health").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().contains_key("x-request-id"),
        "request id is propagated"
    );
    let body = json_body(response).await;
    assert_eq!(body["success"], true);
    assert_eq!(body["code"], "ok");
    assert_eq!(body["message"], "ok");
    assert_eq!(body["data"]["status"], "ok");
    assert!(body.get("timestamp").is_none());
    assert!(body.get("request_id").is_none());
    Ok(())
}

#[tokio::test]
async fn app_builder_http_layer_wraps_routes() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(SharedHttpLayer)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/shared", global_layer_value)
                .get("/plain-error", global_layer_value)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("/shared").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    assert_eq!(body, "shared");

    let response = build_router(&app)?
        .oneshot(Request::get("/plain-error").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let json = json_body(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["code"], "internal_error");
    assert_ne!(json["message"], "private detail");
    Ok(())
}

#[derive(Clone, Default)]
struct TestRateStore(Arc<Mutex<std::collections::HashMap<String, u64>>>);

impl RateLimitStore for TestRateStore {
    fn consume<'a>(
        &'a self,
        key: &'a str,
        _window: Duration,
    ) -> RateLimitFuture<'a, openrust::Result<RateLimitBucket>> {
        Box::pin(async move {
            let mut counts = self
                .0
                .lock()
                .map_err(|_| std::io::Error::other("test store poisoned"))?;
            let count = counts.entry(key.to_owned()).or_default();
            *count += 1;
            Ok(RateLimitBucket::new(*count, 23))
        })
    }
}

#[tokio::test]
async fn route_rate_limit_uses_scoped_custom_keys_and_returns_retry_after() -> TestResult {
    let layer = RateLimitLayer::with_key("posts:list", 1, Duration::from_secs(60), |request| {
        request
            .headers()
            .get("x-test-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or(openrust::Error::BadRequest("missing test key".into()))
    });
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(RateLimiter::new(
            TestRateStore::default(),
        ))))
        .provider(TestRoutes::new(move |routes| {
            routes.get("/limited", rate_limited_ok).layer(layer.layer())
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let first = router
        .clone()
        .oneshot(
            Request::get("/limited")
                .header("x-test-key", "a")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(first.status(), StatusCode::OK);
    let limited = router
        .clone()
        .oneshot(
            Request::get("/limited")
                .header("x-test-key", "a")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        limited
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("23")
    );
    let isolated = router
        .oneshot(
            Request::get("/limited")
                .header("x-test-key", "b")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(isolated.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn rate_limit_and_handler_share_the_request_scoped_factory() -> TestResult {
    struct ScopedLimiter(Arc<AtomicUsize>);
    impl openrust::core::ServiceProvider for ScopedLimiter {
        fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
            let calls = Arc::clone(&self.0);
            app.scoped::<RateLimiter, _>(move |_| {
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(RateLimiter::new(TestRateStore::default())))
                })
            });
            Ok(())
        }
    }
    async fn handler(Inject(_limiter): Inject<RateLimiter>) -> &'static str {
        "ok"
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(ScopedLimiter(Arc::clone(&calls)))
        .provider(TestRoutes::new(|routes| {
            routes.get("/scoped-limit", handler).layer(
                RateLimitLayer::with_key("scope", 10, Duration::from_secs(60), |_| {
                    Ok("one".to_owned())
                })
                .layer(),
            )
        }));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("/scoped-limit").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[cfg(feature = "auth")]
#[tokio::test]
async fn authenticated_rate_limit_rejects_a_missing_identity() -> TestResult {
    let layer = RateLimitLayer::by_user("profile:read", 10, Duration::from_secs(60));
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(RateLimiter::new(
            TestRateStore::default(),
        ))))
        .provider(TestRoutes::new(move |routes| {
            routes
                .get("/limited-user", rate_limited_ok)
                .layer(layer.layer())
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("/limited-user").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn inject_resolves_registered_services() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/greet").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], "hello");
    Ok(())
}

#[tokio::test]
async fn inject_of_unregistered_service_is_a_envelope_not_a_panic() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/missing").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = json_body(response).await;
    assert_eq!(body["code"], "service_not_bound");
    assert_eq!(body["message"], "internal server error");
    Ok(())
}

#[tokio::test]
async fn validated_json_reports_each_field() -> TestResult {
    let request = Request::post("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"ab","pages":0}"#))?;
    let response = router().await?.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
    let body = json_body(response).await;
    assert_eq!(body["code"], "validation_failed");
    assert_eq!(
        body["errors"]["title"][0],
        "title must be at least 3 characters"
    );
    assert_eq!(body["errors"]["pages"][0], "range");
    Ok(())
}

#[tokio::test]
async fn validated_json_accepts_valid_input_and_rejects_malformed_bodies() -> TestResult {
    let ok = Request::post("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"Hello","pages":3}"#))?;
    let response = router().await?.oneshot(ok).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], "Hello");

    let malformed = Request::post("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))?;
    let response = router().await?.oneshot(malformed).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["code"], "bad_request");
    Ok(())
}

#[tokio::test]
async fn panics_become_500_envelopes() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/boom").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["code"], "internal_error");
    Ok(())
}

#[tokio::test]
async fn compression_negotiates_gzip_and_br_and_respects_absence() -> TestResult {
    for (accept, expected) in [
        (Some("gzip"), Some("gzip")),
        (Some("br"), Some("br")),
        (None, None),
    ] {
        let mut request = Request::get("/api/large");
        if let Some(accept) = accept {
            request = request.header(header::ACCEPT_ENCODING, accept);
        }
        let response = router()
            .await?
            .oneshot(request.body(Body::empty())?)
            .await?;
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            expected
        );
        let bytes = to_bytes(response.into_body(), usize::MAX).await?;
        if expected.is_some() {
            assert!(bytes.len() < 2200);
        } else {
            let expected_body = serde_json::json!({
                "success": true,
                "code": "ok",
                "message": "ok",
                "data": "compressible response ".repeat(100),
            });
            assert_eq!(bytes.len(), serde_json::to_vec(&expected_body)?.len());
        }
    }
    Ok(())
}

#[tokio::test]
async fn public_site_serves_index_and_maps_files_from_configured_directory() -> TestResult {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("index.html"), "site home")?;
    std::fs::write(directory.path().join("app.js"), "site asset")?;
    let mut config = FrameworkConfig::default();
    config.server.public_dir = directory.path().to_owned();
    let app = router_with_exact_config(config).await?;

    let home = app
        .clone()
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    assert_eq!(home.status(), StatusCode::OK);
    assert_eq!(home.headers()[header::CONTENT_TYPE], "text/html");
    assert_eq!(to_bytes(home.into_body(), usize::MAX).await?, "site home");

    let asset = app
        .oneshot(Request::get("/app.js").body(Body::empty())?)
        .await?;
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(to_bytes(asset.into_body(), usize::MAX).await?, "site asset");
    Ok(())
}

#[tokio::test]
async fn root_target_supports_internal_forward_and_redirect() -> TestResult {
    let mut forward = FrameworkConfig::default();
    forward.server.root_target = "forward:/api/greet".to_owned();
    let response = router_with_exact_config(forward)
        .await?
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], "hello");

    let mut redirect = FrameworkConfig::default();
    redirect.server.root_target = "redirect:https://example.com/home".to_owned();
    let response = router_with_exact_config(redirect)
        .await?
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        response.headers()[header::LOCATION],
        "https://example.com/home"
    );
    Ok(())
}

#[tokio::test]
async fn invalid_root_target_is_rejected_during_router_build() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.root_target = "redirect:javascript:alert(1)".to_owned();
    let error = router_with_exact_config(config)
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("unsafe redirect scheme must be rejected"))?;
    assert!(error.to_string().contains("server.root_target"));
    Ok(())
}

#[tokio::test]
async fn static_fallback_does_not_capture_api_or_websocket_namespaces() -> TestResult {
    let app = router().await?;
    for path in ["/api/unknown", "/ws/unknown"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(json_body(response).await["code"], "not_found");
    }
    Ok(())
}

#[tokio::test]
async fn disabled_compression_ignores_accept_encoding() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.compression.enabled = false;
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/api/large")
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())?,
        )
        .await?;
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    Ok(())
}

#[tokio::test]
async fn cors_is_disabled_by_default() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    Ok(())
}

#[tokio::test]
async fn cors_allows_listed_origin_and_preflight_with_request_id() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    config.server.cors.expose_headers = vec!["x-total-count".into()];
    let router = router_with_config(config).await?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"https://example.com".parse()?)
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_EXPOSE_HEADERS),
        Some(&"x-total-count".parse()?)
    );

    let preflight = router
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/health")
                .header(header::ORIGIN, "https://example.com")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(preflight.status(), StatusCode::OK);
    assert!(preflight.headers().contains_key("x-request-id"));
    assert_eq!(
        preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS),
        Some(&"GET,POST,PUT,PATCH,DELETE,OPTIONS".parse()?)
    );
    assert_eq!(
        preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS),
        Some(&"content-type,authorization,x-request-id".parse()?)
    );
    assert_eq!(
        preflight.headers().get(header::ACCESS_CONTROL_MAX_AGE),
        Some(&"600".parse()?)
    );
    assert!(
        to_bytes(preflight.into_body(), usize::MAX)
            .await?
            .is_empty()
    );

    let denied = router
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://other.example")
                .body(Body::empty())?,
        )
        .await?;
    assert!(
        !denied
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    Ok(())
}

#[tokio::test]
async fn cors_preflight_rejects_unlisted_origin() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/health")
                .header(header::ORIGIN, "https://other.example")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .body(Body::empty())?,
        )
        .await?;
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    assert!(to_bytes(response.into_body(), usize::MAX).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn cors_origin_error_is_wrapped_outside_preflight() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/api/not-found")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"https://example.com".parse()?)
    );
    let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
    let body = json_body(response).await;
    assert_eq!(body["success"], false);
    assert_eq!(body["code"], "not_found");
    assert!(body.get("data").is_none());
    assert_eq!(body["request_id"], request_id);
    Ok(())
}

#[tokio::test]
async fn cors_timeout_retains_allow_origin() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.request_timeout_secs = 1;
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/api/slow")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"https://example.com".parse()?)
    );
    assert_eq!(json_body(response).await["code"], "request_timeout");
    Ok(())
}

#[tokio::test]
async fn cors_wildcard_and_credentials_have_expected_headers() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["*".into()];
    let response = router_with_config(config.clone())
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"*".parse()?)
    );

    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    config.server.cors.allow_credentials = true;
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        Some(&"true".parse()?)
    );
    Ok(())
}

#[tokio::test]
async fn invalid_cors_config_returns_keyed_errors() {
    let cases = [
        (vec![], false, "server.cors.allowed_origins"),
        (
            vec!["*", "https://example.com"],
            false,
            "server.cors.allowed_origins",
        ),
        (vec!["*"], true, "server.cors.allow_credentials"),
        (
            vec!["https://example.com", "bad\norigin"],
            false,
            "server.cors.allowed_origins[1]",
        ),
    ];
    for (origins, credentials, key) in cases {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = origins.into_iter().map(str::to_owned).collect();
        config.server.cors.allow_credentials = credentials;
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains(key), "{error}");
    }
}

#[tokio::test]
async fn invalid_cors_origin_reports_escaped_value() {
    for origin in [
        "bad\norigin",
        "https://example.com/",
        "https://example.com/path",
        "https://example.com?x=1",
        "https://example.com:bad",
        "example.com",
    ] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec![origin.into()];
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.allowed_origins[0]: invalid origin {origin:?}"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn cors_wildcards_with_credentials_return_errors() {
    for key in ["allowed_methods", "allowed_headers", "expose_headers"] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec!["https://example.com".into()];
        config.server.cors.allow_credentials = true;
        match key {
            "allowed_methods" => config.server.cors.allowed_methods = vec!["*".into()],
            "allowed_headers" => config.server.cors.allowed_headers = vec!["*".into()],
            _ => config.server.cors.expose_headers = vec!["*".into()],
        }
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.{key}[0]: cannot be \"*\" when server.cors.allow_credentials = true"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn cors_mixed_wildcards_return_errors() {
    for key in ["allowed_methods", "allowed_headers", "expose_headers"] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec!["https://example.com".into()];
        match key {
            "allowed_methods" => {
                config.server.cors.allowed_methods = vec!["GET".into(), "*".into()];
            }
            "allowed_headers" => {
                config.server.cors.allowed_headers = vec!["content-type".into(), "*".into()];
            }
            _ => config.server.cors.expose_headers = vec!["x-total-count".into(), "*".into()],
        }
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.{key}[1]: \"*\" cannot be mixed with concrete values"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn invalid_cors_methods_and_headers_return_keyed_errors() {
    for key in ["allowed_methods", "allowed_headers", "expose_headers"] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec!["https://example.com".into()];
        let invalid_value = if key == "allowed_methods" {
            config.server.cors.allowed_methods = vec!["BAD METHOD".into()];
            "BAD METHOD"
        } else if key == "allowed_headers" {
            config.server.cors.allowed_headers = vec!["bad header".into()];
            "bad header"
        } else {
            config.server.cors.expose_headers = vec!["bad header".into()];
            "bad header"
        };
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.{key}[0]: invalid value {invalid_value:?}"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn disabled_cors_ignores_other_invalid_fields() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.allowed_origins = vec!["bad\norigin".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn json_syntax_is_400_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{broken"))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["code"], "bad_request");
    Ok(())
}

#[tokio::test]
async fn json_content_type_is_415_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::post("/api/raw").body(Body::from("{}"))?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(json_body(response).await["code"], "unsupported_media_type");
    Ok(())
}

#[tokio::test]
async fn json_field_type_is_422_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"title":"hello","pages":"wrong"}"#))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["code"], "validation_failed");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|detail| detail.contains("pages"))
    );
    assert!(body["errors"]["pages"].is_array(), "{body}");
    Ok(())
}

#[tokio::test]
async fn oversized_json_is_413_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(vec![b' '; 3_000_000]))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json_body(response).await["code"], "payload_too_large");
    Ok(())
}

#[tokio::test]
async fn path_and_query_parse_errors_are_400_envelopes() -> TestResult {
    for uri in ["/api/items/nope", "/api/search?title=x&pages=nope"] {
        let response = router()
            .await?
            .oneshot(Request::get(uri).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["code"], "bad_request");
    }
    Ok(())
}

#[tokio::test]
async fn unknown_route_is_404_envelope_with_request_id() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/unknown").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.headers().contains_key("x-request-id"));
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let body = json_body(response).await;
    assert_eq!(body["code"], "not_found");
    assert!(is_utc_rfc3339(&body["timestamp"]));
    Ok(())
}

#[tokio::test]
async fn wrong_method_is_405_envelope_with_allow() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::post("/api/greet").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(response.headers().contains_key(header::ALLOW));
    assert_eq!(json_body(response).await["code"], "method_not_allowed");
    Ok(())
}

#[tokio::test]
async fn handler_404_keeps_its_detail() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/own-404").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_body(response).await;
    assert_eq!(body["message"], "my resource is absent");
    Ok(())
}

#[tokio::test]
async fn cors_applies_to_fallback_envelopes() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let router = router_with_config(config).await?;
    for (method, uri, status) in [
        ("GET", "/nope", StatusCode::NOT_FOUND),
        ("POST", "/api/greet", StatusCode::METHOD_NOT_ALLOWED),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::ORIGIN, "https://example.com")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://example.com"
        );
        assert!(response.headers().contains_key("x-request-id"));
    }
    Ok(())
}

#[tokio::test]
async fn missing_json_field_is_422_with_field_error() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pages":1}"#))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["code"], "validation_failed");
    assert!(body["errors"]["title"].is_array(), "{body}");
    Ok(())
}

#[tokio::test]
async fn always_ok_wraps_success_errors_and_fallbacks_with_matching_request_id() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::AlwaysOk;
    let router = router_with_exact_config(config).await?;
    for (method, uri, expected_code) in [
        ("GET", "/api/page", "ok"),
        ("POST", "/api/created", "ok"),
        ("DELETE", "/api/no-content", "ok"),
        ("GET", "/nope", "not_found"),
        ("POST", "/api/greet", "method_not_allowed"),
        ("GET", "/api/boom", "internal_error"),
        ("GET", "/api/limited", "rate_limited"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("x-request-id", "client-123")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(response.headers()["x-request-id"], "client-123");
        if uri == "/api/created" {
            assert_eq!(response.headers()[header::LOCATION], "/api/items/42");
        }
        if uri == "/api/limited" {
            assert_eq!(response.headers()[header::RETRY_AFTER], "30");
        }
        if method == "POST" && uri == "/api/greet" {
            assert!(response.headers().contains_key(header::ALLOW));
        }
        let body = json_body(response).await;
        assert_eq!(body["code"], expected_code, "{uri}");
        assert_eq!(body["success"], expected_code == "ok", "{uri}");
        if expected_code == "ok" {
            assert!(body.get("request_id").is_none(), "{uri}");
            assert!(body.get("timestamp").is_none(), "{uri}");
        } else {
            assert_eq!(body["request_id"], "client-123", "{uri}");
            assert!(is_utc_rfc3339(&body["timestamp"]), "{uri}");
        }
        if uri == "/api/no-content" || expected_code != "ok" {
            assert!(body.get("data").is_none(), "{uri}");
        }
        if uri == "/api/page" {
            assert_eq!(body["data"], serde_json::json!([1, 2]));
            assert_eq!(body["meta"]["total_pages"], 3);
        }
        if uri == "/api/boom" {
            assert_eq!(body["message"], "internal server error");
        }
    }
    Ok(())
}

#[tokio::test]
async fn http_execution_context_contains_request_id_budget_and_timeout_cancels_children()
-> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.request_timeout_secs = 1;
    let router = router_with_exact_config(config).await?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/api/execution")
                .header("x-request-id", "scope-123")
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(response).await;
    assert_eq!(body["data"]["request_id"], "scope-123");
    assert_eq!(body["data"]["has_budget"], true);

    REQUEST_CANCELLED.store(false, Ordering::SeqCst);
    let response = router
        .oneshot(Request::get("/api/cancel").body(Body::empty())?)
        .await?;
    assert_eq!(json_body(response).await["code"], "request_timeout");
    tokio::time::timeout(Duration::from_millis(100), async {
        while !REQUEST_CANCELLED.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn semantic_keeps_created_error_and_rate_limit_statuses() -> TestResult {
    let router = router().await?;
    for (method, uri, expected) in [
        ("POST", "/api/created", StatusCode::CREATED),
        ("DELETE", "/api/no-content", StatusCode::OK),
        ("GET", "/nope", StatusCode::NOT_FOUND),
        ("POST", "/api/greet", StatusCode::METHOD_NOT_ALLOWED),
        ("GET", "/api/limited", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), expected, "{uri}");
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        let body = json_body(response).await;
        if expected.is_success() {
            assert!(body.get("request_id").is_none());
            assert!(body.get("timestamp").is_none());
        } else {
            assert_eq!(body["request_id"], request_id);
            assert!(is_utc_rfc3339(&body["timestamp"]));
        }
    }
    Ok(())
}

#[tokio::test]
async fn always_ok_maps_extractor_errors_and_timeout_to_envelopes() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.request_timeout_secs = 1;
    config.api.http_status = openrust::config::HttpStatusMode::AlwaysOk;
    let router = router_with_exact_config(config).await?;
    let cases = [
        (
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{bad"))?,
            "bad_request",
        ),
        (
            Request::post("/api/raw").body(Body::from("{}"))?,
            "unsupported_media_type",
        ),
        (
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pages":1}"#))?,
            "validation_failed",
        ),
        (
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!("\"{}\"", "x".repeat(2_100_000))))?,
            "payload_too_large",
        ),
        (
            Request::get("/api/slow").body(Body::empty())?,
            "request_timeout",
        ),
    ];
    for (request, code) in cases {
        let response = router.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK, "{code}");
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        let body = json_body(response).await;
        assert_eq!(body["request_id"], request_id, "{code}");
        assert_eq!(body["code"], code);
        assert_eq!(body["success"], false);
    }
    Ok(())
}

#[tokio::test]
async fn default_status_mode_uses_semantic_http_codes() -> TestResult {
    assert_eq!(
        FrameworkConfig::default().api.http_status,
        openrust::config::HttpStatusMode::Semantic
    );
    let router = router_with_exact_config(FrameworkConfig::default()).await?;
    for (method, uri, expected_status, expected_code) in [
        ("POST", "/api/created", StatusCode::CREATED, "ok"),
        ("GET", "/nope", StatusCode::NOT_FOUND, "not_found"),
        (
            "POST",
            "/api/greet",
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ),
        (
            "GET",
            "/api/boom",
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
        ),
        (
            "GET",
            "/api/limited",
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
        ),
        (
            "POST",
            "/api/raw",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), expected_status, "{uri}");
        let body = json_body(response).await;
        assert_eq!(body["success"], expected_status.is_success(), "{uri}");
        assert_eq!(body["code"], expected_code, "{uri}");
    }
    let validation = router
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pages":1}"#))?,
        )
        .await?;
    assert_eq!(validation.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(validation).await;
    assert_eq!(body["success"], false);
    assert_eq!(body["code"], "validation_failed");
    assert!(body["errors"]["title"].is_array());
    Ok(())
}

struct FailingI18nProvider;
impl openrust::core::ServiceProvider for FailingI18nProvider {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
        app.scoped::<openrust::i18n::I18n, _>(|_| {
            Box::pin(async {
                Err(openrust::Error::internal(std::io::Error::other(
                    "i18n service unavailable",
                )))
            })
        });
        Ok(())
    }
}

#[tokio::test]
async fn i18n_resolution_errors_keep_request_status_mode_and_id() -> TestResult {
    for (mode, expected_status) in [
        (
            openrust::config::HttpStatusMode::Semantic,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (openrust::config::HttpStatusMode::AlwaysOk, StatusCode::OK),
    ] {
        let mut config = FrameworkConfig::default();
        config.api.http_status = mode;
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
            .merge(figment::Figment::new());
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(FailingI18nProvider);
        let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
        let response = build_router(&app)?
            .oneshot(
                Request::get("/unreachable")
                    .header("x-request-id", "i18n-resolution-failure")
                    .body(Body::empty())?,
            )
            .await?;

        assert_eq!(response.status(), expected_status);
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        assert_eq!(request_id, "i18n-resolution-failure");
        let body = json_body(response).await;
        assert_eq!(body["request_id"], request_id);
        assert_eq!(body["code"], "service_initialization_failed");
    }
    Ok(())
}

struct TestInstance<T: Send + Sync + 'static>(std::sync::Arc<T>);
impl<T: Send + Sync + 'static> openrust::core::ServiceProvider for TestInstance<T> {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
        app.instance(std::sync::Arc::clone(&self.0));
        Ok(())
    }
}

struct TestRoutes<F>(std::sync::Arc<F>);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes))
    }
}
impl<F> openrust::core::ServiceProvider for TestRoutes<F>
where
    F: Fn(openrust::http::Routes) -> openrust::http::Routes + Send + Sync + 'static,
{
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let routes = std::sync::Arc::clone(&self.0);
        Box::pin(async move {
            app.routes(move |value| routes(value))?;
            Ok(())
        })
    }
}

type IdentityFactory = dyn Fn(
        openrust::core::container::Resolver,
    ) -> openrust::BoxFuture<'static, openrust::Result<Arc<RequestIdentity>>>
    + Send
    + Sync;
struct IdentityProvider(std::sync::Arc<IdentityFactory>);
impl IdentityProvider {
    fn new(
        factory: impl Fn(
            openrust::core::container::Resolver,
        )
            -> openrust::BoxFuture<'static, openrust::Result<Arc<RequestIdentity>>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(factory))
    }
}
impl openrust::core::ServiceProvider for IdentityProvider {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
        let factory = Arc::clone(&self.0);
        app.scoped::<RequestIdentity, _>(move |resolver| factory(resolver));
        Ok(())
    }
}
struct SharedHttpLayer;
impl openrust::core::ServiceProvider for SharedHttpLayer {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.http_layer(axum::middleware::from_fn(add_shared_extension))?;
            Ok(())
        })
    }
}

// 以下 fixture 经生产 Provider/Router 装配后绑定真实 TCP listener；不以 oneshot 代替断连。
type NetworkResult<T> = Result<T, Box<dyn std::error::Error>>;

struct NetworkScopedResource(usize);
struct NetworkScopedIdentity(usize);
struct NetworkScopeProbe {
    next: AtomicUsize,
    cleanup_starts: AtomicUsize,
    cleanup_finishes: AtomicUsize,
    identity_barrier: tokio::sync::Barrier,
    release: tokio::sync::Semaphore,
    started: tokio::sync::mpsc::UnboundedSender<usize>,
    completed: tokio::sync::mpsc::UnboundedSender<usize>,
    scopes: tokio::sync::mpsc::UnboundedSender<Arc<Scope>>,
}
struct NetworkScopeEvents {
    started: tokio::sync::mpsc::UnboundedReceiver<usize>,
    completed: tokio::sync::mpsc::UnboundedReceiver<usize>,
    scopes: tokio::sync::mpsc::UnboundedReceiver<Arc<Scope>>,
}
struct NetworkScopeProvider(Arc<NetworkScopeProbe>);
impl openrust::core::ServiceProvider for NetworkScopeProvider {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
        app.instance(Arc::clone(&self.0));
        app.scoped::<NetworkScopedResource, _>(|resolver| {
            Box::pin(async move {
                let probe = resolver.resolve::<NetworkScopeProbe>().await?;
                let id = probe.next.fetch_add(1, Ordering::SeqCst) + 1;
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        probe.cleanup_starts.fetch_add(1, Ordering::SeqCst);
                        probe.started.send(id).map_err(openrust::Error::internal)?;
                        let permit = probe
                            .release
                            .acquire()
                            .await
                            .map_err(openrust::Error::internal)?;
                        permit.forget();
                        probe.cleanup_finishes.fetch_add(1, Ordering::SeqCst);
                        probe
                            .completed
                            .send(id)
                            .map_err(openrust::Error::internal)?;
                        Ok(())
                    })
                })?;
                Ok(Arc::new(NetworkScopedResource(id)))
            })
        });
        app.scoped::<NetworkScopedIdentity, _>(|resolver| {
            Box::pin(async move {
                let probe = resolver.resolve::<NetworkScopeProbe>().await?;
                Ok(Arc::new(NetworkScopedIdentity(
                    probe.next.fetch_add(1, Ordering::SeqCst) + 1,
                )))
            })
        });
        Ok(())
    }
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/scope-stream", network_scope_stream)
                    .get("/scope-sse", network_scope_sse)
                    .get("/scope-identity", network_scope_identity)
            })?;
            Ok(())
        })
    }
}

async fn network_scope_stream(
    Inject(resource): Inject<NetworkScopedResource>,
    Inject(probe): Inject<NetworkScopeProbe>,
    axum::extract::State(state): axum::extract::State<openrust::AppState>,
    Extension(scope): Extension<Arc<Scope>>,
) -> openrust::Result<Response> {
    probe
        .scopes
        .send(scope)
        .map_err(openrust::Error::internal)?;
    let first = futures_util::stream::once(async move {
        let body_resource = state
            .resolve::<NetworkScopedResource>()
            .await
            .map_err(std::io::Error::other)?;
        if !Arc::ptr_eq(&resource, &body_resource) {
            return Err(std::io::Error::other(
                "stream body resolved a different request scope",
            ));
        }
        Ok::<_, std::io::Error>(bytes::Bytes::from(format!("scope={}\n", resource.0)))
    });
    Ok(Body::from_stream(first.chain(futures_util::stream::pending())).into_response())
}

async fn network_scope_sse(
    Inject(resource): Inject<NetworkScopedResource>,
    Inject(probe): Inject<NetworkScopeProbe>,
    axum::extract::State(state): axum::extract::State<openrust::AppState>,
    Extension(scope): Extension<Arc<Scope>>,
) -> openrust::Result<Response> {
    use axum::response::sse::{Event, Sse};
    probe
        .scopes
        .send(scope)
        .map_err(openrust::Error::internal)?;
    let first = futures_util::stream::once(async move {
        let body_resource = state
            .resolve::<NetworkScopedResource>()
            .await
            .map_err(std::io::Error::other)?;
        if !Arc::ptr_eq(&resource, &body_resource) {
            return Err(std::io::Error::other(
                "SSE body resolved a different request scope",
            ));
        }
        Ok::<_, std::io::Error>(Event::default().data(format!("scope={}", resource.0)))
    });
    Ok(Sse::new(first.chain(futures_util::stream::pending())).into_response())
}

async fn network_scope_identity(
    Inject(identity): Inject<NetworkScopedIdentity>,
    Inject(probe): Inject<NetworkScopeProbe>,
    axum::extract::State(state): axum::extract::State<openrust::AppState>,
) -> openrust::Result<axum::Json<serde_json::Value>> {
    let _ = probe.identity_barrier.wait().await;
    let again = state.resolve::<NetworkScopedIdentity>().await?;
    let request_id = ExecutionContext::current()
        .and_then(|context| context.metadata().request_id().map(str::to_owned));
    let plain = tokio::spawn(async move {
        let inherited = ExecutionContext::current().is_some();
        let root = state.resolve::<NetworkScopedIdentity>().await?;
        Ok::<_, openrust::Error>((inherited, root.0))
    })
    .await
    .map_err(openrust::Error::internal)??;
    Ok(axum::Json(serde_json::json!({
        "id":identity.0, "again":again.0, "request_id":request_id,
        "plain_inherited":plain.0, "plain_id":plain.1,
    })))
}

struct NetworkScopeServer {
    app: Arc<openrust::App>,
    address: std::net::SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl NetworkScopeServer {
    async fn start() -> NetworkResult<(Self, Arc<NetworkScopeProbe>, NetworkScopeEvents)> {
        let (started, started_rx) = tokio::sync::mpsc::unbounded_channel();
        let (completed, completed_rx) = tokio::sync::mpsc::unbounded_channel();
        let (scopes, scopes_rx) = tokio::sync::mpsc::unbounded_channel();
        let probe = Arc::new(NetworkScopeProbe {
            next: AtomicUsize::new(0),
            cleanup_starts: AtomicUsize::new(0),
            cleanup_finishes: AtomicUsize::new(0),
            identity_barrier: tokio::sync::Barrier::new(2),
            release: tokio::sync::Semaphore::new(0),
            started,
            completed,
            scopes,
        });
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(NetworkScopeProvider(Arc::clone(&probe)));
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let app = Arc::new(openrust::App::from_provider_runtime(
            builder.start(sources).await?,
        )?);
        let router = build_router(&app)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (stop, signal) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = signal.await;
                })
                .await
        });
        Ok((
            Self {
                app,
                address,
                stop,
                task,
            },
            probe,
            NetworkScopeEvents {
                started: started_rx,
                completed: completed_rx,
                scopes: scopes_rx,
            },
        ))
    }
    async fn finish(self) -> NetworkResult<()> {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(5), self.task).await???;
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        Ok(())
    }
}

async fn open_scope_stream(
    address: std::net::SocketAddr,
    path: &str,
) -> NetworkResult<tokio::net::TcpStream> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut socket = tokio::net::TcpStream::connect(address).await?;
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut buffer = [0_u8; 2048];
        loop {
            let read = socket.read(&mut buffer).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "stream ended before first event",
                ));
            }
            response.extend_from_slice(&buffer[..read]);
            if response
                .windows(b"scope=".len())
                .any(|part| part == b"scope=")
            {
                return Ok(());
            }
        }
    })
    .await??;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    if path == "/scope-sse" {
        assert!(String::from_utf8_lossy(&response).contains("text/event-stream"));
    }
    Ok(socket)
}

async fn scope_event<T>(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<T>,
) -> NetworkResult<T> {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await?
        .ok_or_else(|| "scope event channel closed".into())
}

#[tokio::test]
async fn real_tcp_stream_and_sse_keep_scope_until_disconnect_and_cleanup_once() -> NetworkResult<()>
{
    for path in ["/scope-stream", "/scope-sse"] {
        let (server, probe, mut events) = NetworkScopeServer::start().await?;
        let socket = open_scope_stream(server.address, path).await?;
        let scope = scope_event(&mut events.scopes).await?;
        let resource = scope.resolve::<NetworkScopedResource>().await?;
        assert_eq!(probe.cleanup_starts.load(Ordering::SeqCst), 0);
        drop(socket);
        assert_eq!(scope_event(&mut events.started).await?, resource.0);
        assert_eq!(
            scope
                .resolve::<NetworkScopedResource>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("scope_closed".to_owned())
        );
        probe.release.add_permits(1);
        assert_eq!(scope_event(&mut events.completed).await?, resource.0);
        server.finish().await?;
        assert_eq!(probe.cleanup_starts.load(Ordering::SeqCst), 1);
        assert_eq!(probe.cleanup_finishes.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn real_tcp_app_shutdown_waits_for_disconnected_scope_async_cleanup() -> NetworkResult<()> {
    use futures_util::FutureExt as _;
    let (server, probe, mut events) = NetworkScopeServer::start().await?;
    let socket = open_scope_stream(server.address, "/scope-stream").await?;
    let scope = scope_event(&mut events.scopes).await?;
    let id = scope.resolve::<NetworkScopedResource>().await?.0;
    drop(socket);
    assert_eq!(scope_event(&mut events.started).await?, id);
    let app = Arc::clone(&server.app);
    let shutdown = app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5));
    tokio::pin!(shutdown);
    assert!(
        shutdown.as_mut().now_or_never().is_none(),
        "shutdown must wait for the registered cleanup"
    );
    assert_eq!(probe.cleanup_finishes.load(Ordering::SeqCst), 0);
    probe.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), shutdown).await??;
    assert_eq!(scope_event(&mut events.completed).await?, id);
    server.finish().await?;
    assert_eq!(probe.cleanup_starts.load(Ordering::SeqCst), 1);
    assert_eq!(probe.cleanup_finishes.load(Ordering::SeqCst), 1);
    Ok(())
}

async fn request_scope_identity(
    address: std::net::SocketAddr,
    request_id: &str,
) -> NetworkResult<serde_json::Value> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut socket = tokio::net::TcpStream::connect(address).await?;
    socket.write_all(format!("GET /scope-identity HTTP/1.1\r\nHost: {address}\r\nx-request-id: {request_id}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut bytes)).await??;
    assert!(bytes.starts_with(b"HTTP/1.1 200"));
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or("HTTP headers missing")?;
    Ok(serde_json::from_slice(&bytes[split + 4..])?)
}

#[tokio::test]
async fn real_tcp_concurrent_requests_isolate_scope_and_plain_spawn_identity() -> NetworkResult<()>
{
    let (server, _, _) = NetworkScopeServer::start().await?;
    let (left, right) = tokio::join!(
        request_scope_identity(server.address, "scope-network-left"),
        request_scope_identity(server.address, "scope-network-right"),
    );
    let (left, right) = (left?, right?);
    assert_ne!(left["id"], right["id"]);
    assert_eq!(left["request_id"], "scope-network-left");
    assert_eq!(right["request_id"], "scope-network-right");
    for body in [&left, &right] {
        assert_eq!(body["id"], body["again"]);
        assert_eq!(body["plain_inherited"], false);
        assert_ne!(body["id"], body["plain_id"]);
    }
    assert_eq!(left["plain_id"], right["plain_id"]);
    server.finish().await
}
