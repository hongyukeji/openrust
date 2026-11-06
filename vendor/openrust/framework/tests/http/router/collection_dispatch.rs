//! 最终集合的真实 handler/middleware 分派，不以 URL 索引存在代替执行证明。
use super::*;
use openrust::http::RequestInfo;
use std::collections::BTreeMap;

type CaseResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct Calls {
    old: AtomicUsize,
    old_middleware: AtomicUsize,
    fallback: AtomicUsize,
}

fn snapshot(info: &RequestInfo, marker: &str) -> openrust::Result<Response> {
    let route = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("collection_route_missing", "实际路由缺失"))?;
    let mut response = Json(serde_json::json!({
        "marker":marker, "action":route.action(), "name":route.name(),
        "uri":route.uri(), "methods":route.methods(), "parameters":route.parameters(),
    }))
    .into_response();
    for (key, value) in [
        ("x-collection-marker", marker.to_owned()),
        (
            "x-collection-action",
            route.action().unwrap_or("anonymous").to_owned(),
        ),
        ("x-collection-methods", route.methods().join(",")),
    ] {
        response
            .headers_mut()
            .insert(key, value.parse().map_err(openrust::Error::internal)?);
    }
    Ok(response)
}

async fn live(info: RequestInfo) -> openrust::Result<Response> {
    snapshot(&info, "live")
}
async fn old(
    info: RequestInfo,
    Extension(calls): Extension<Arc<Calls>>,
) -> openrust::Result<Response> {
    calls.old.fetch_add(1, Ordering::SeqCst);
    snapshot(&info, "old")
}
async fn old_middleware(
    Extension(calls): Extension<Arc<Calls>>,
    request: Request,
    next: Next,
) -> Response {
    calls.old_middleware.fetch_add(1, Ordering::SeqCst);
    next.run(request).await
}
async fn fallback(
    info: RequestInfo,
    Extension(calls): Extension<Arc<Calls>>,
) -> openrust::Result<Response> {
    calls.fallback.fetch_add(1, Ordering::SeqCst);
    snapshot(&info, "fallback")
}
async fn business_404(info: RequestInfo) -> openrust::Result<Response> {
    let mut response = snapshot(&info, "business-404")?;
    *response.status_mut() = StatusCode::NOT_FOUND;
    Ok(response)
}
async fn business_405(info: RequestInfo) -> openrust::Result<Response> {
    let mut response = snapshot(&info, "business-405")?;
    *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    Ok(response)
}

async fn build_app<F>(routes: F) -> CaseResult<(openrust::Application, Arc<Calls>)>
where
    F: Fn(Router) -> Router + Send + Sync + 'static,
{
    let mut config = FrameworkConfig::default();
    config.app.url = "http://localhost".into();
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(routes));
    let app = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(config),
        ))
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    Ok((app, Arc::new(Calls::default())))
}
async fn build<F>(routes: F) -> CaseResult<(axum::Router, Arc<Calls>)>
where
    F: Fn(Router) -> Router + Send + Sync + 'static,
{
    let (app, calls) = build_app(routes).await?;
    Ok((
        build_router(&app)?.layer(Extension(Arc::clone(&calls))),
        calls,
    ))
}
async fn send(router: &axum::Router, host: &str, method: &str, path: &str) -> CaseResult<Response> {
    Ok(router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, host)
                .header(header::ACCEPT, "application/json")
                .body(Body::empty())?,
        )
        .await?)
}
fn no_old(calls: &Calls) {
    assert_eq!(
        calls.old.load(Ordering::SeqCst),
        0,
        "被集合淘汰的旧 handler 不应执行"
    );
    assert_eq!(
        calls.old_middleware.load(Ordering::SeqCst),
        0,
        "旧路由 middleware 不应先执行再被忽略"
    );
}
fn marker(response: &Response, expected: &str) {
    assert_eq!(response.headers()["x-collection-marker"], expected);
}

#[tokio::test]
async fn collection_raw_domain_collision_removes_old_handler_and_middleware() -> CaseResult {
    for anonymous in [false, true] {
        let (router, calls) = build(move |routes| {
            let routes = routes.group(
                RouteGroup::new()
                    .domain("a.test")
                    .middleware(axum::middleware::from_fn(old_middleware)),
                |routes| {
                    if anonymous {
                        routes.get("/bc", old)
                    } else {
                        routes.get_action("/bc", "Old@show", old)
                    }
                },
            );
            routes.group(RouteGroup::new().domain("a.testb"), |routes| {
                if anonymous {
                    routes.get("/c", live)
                } else {
                    routes.get_action("/c", "New@show", live)
                }
            })
        })
        .await?;
        for method in ["GET", "HEAD", "POST", "OPTIONS"] {
            let removed = send(&router, "a.test", method, "/bc").await?;
            assert_eq!(removed.status(), StatusCode::NOT_FOUND);
            assert!(!removed.headers().contains_key(header::ALLOW));
        }
        for method in ["GET", "HEAD"] {
            let current = send(&router, "a.testb", method, "/c").await?;
            assert_eq!(current.status(), StatusCode::OK);
            marker(&current, "live");
            assert_eq!(
                current.headers()["x-collection-action"],
                if anonymous { "anonymous" } else { "New@show" }
            );
        }
        let post = send(&router, "a.testb", "POST", "/c").await?;
        assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(post.headers()[header::ALLOW], "GET, HEAD");
        let options = send(&router, "a.testb", "OPTIONS", "/c").await?;
        assert_eq!(options.status(), StatusCode::OK);
        assert_eq!(options.headers()[header::ALLOW], "GET,HEAD");
        no_old(&calls);
    }
    assert_collection_over_tcp().await?;
    Ok(())
}

#[tokio::test]
async fn collection_domain_union_hides_anonymous_before_host_and_excludes_allow() -> CaseResult {
    let (router, calls) = build(|routes| {
        routes
            .group(
                RouteGroup::new().middleware(axum::middleware::from_fn(old_middleware)),
                |routes| routes.get("/a.testbc", old),
            )
            .group(RouteGroup::new().domain("a.test"), |routes| {
                routes.get("/bc", live)
            })
    })
    .await?;
    for method in ["GET", "HEAD", "POST", "OPTIONS"] {
        let response = send(&router, "other.test", method, "/a.testbc").await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!response.headers().contains_key(header::ALLOW));
    }
    marker(&send(&router, "a.test", "GET", "/bc").await?, "live");
    no_old(&calls);
    Ok(())
}

#[tokio::test]
async fn collection_surviving_post_has_get_head_405_and_options_from_live_methods() -> CaseResult {
    let (router, calls) = build(|routes| {
        routes
            .group(
                RouteGroup::new().middleware(axum::middleware::from_fn(old_middleware)),
                |routes| routes.get("/a.testbc", old),
            )
            .post("/a.testbc", live)
            .group(RouteGroup::new().domain("a.test"), |routes| {
                routes.get("/bc", live)
            })
    })
    .await?;
    for method in ["GET", "HEAD"] {
        let response = send(&router, "other.test", method, "/a.testbc").await?;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], "POST");
    }
    let options = send(&router, "other.test", "OPTIONS", "/a.testbc").await?;
    assert_eq!(options.status(), StatusCode::OK);
    assert_eq!(options.headers()[header::ALLOW], "POST");
    assert!(to_bytes(options.into_body(), 4096).await?.is_empty());
    marker(
        &send(&router, "other.test", "POST", "/a.testbc").await?,
        "live",
    );
    no_old(&calls);
    Ok(())
}

async fn old_urls(urls: UrlGenerator) -> openrust::Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "split":urls.action("SplitOld@show", &BTreeMap::new())?,
        "partial":urls.action("PartialOld@show", &BTreeMap::new())?,
    })))
}

#[tokio::test]
async fn collection_split_partial_preserve_distinct_action_and_dispatch_views() -> CaseResult {
    let (router, calls) = build(|routes| {
        routes
            .on_action("/split", &["GET", "POST"], "SplitOld@show", old)
            .get_action("/split", "SplitNew@get", live)
            .on_action("/split", &["POST"], "SplitNew@post", live)
            .on_action("/partial", &["GET", "POST"], "PartialOld@show", old)
            .get_action("/partial", "PartialNew@get", live)
            .get("/urls", old_urls)
    })
    .await?;
    for (method, action, methods) in [
        ("GET", "SplitNew@get", "GET,HEAD"),
        ("HEAD", "SplitNew@get", "GET,HEAD"),
        ("POST", "SplitNew@post", "POST"),
    ] {
        let response = send(&router, "localhost", method, "/split").await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-collection-action"], action);
        assert_eq!(response.headers()["x-collection-methods"], methods);
    }
    no_old(&calls);
    let get = send(&router, "localhost", "GET", "/partial").await?;
    assert_eq!(get.headers()["x-collection-action"], "PartialNew@get");
    let post = send(&router, "localhost", "POST", "/partial").await?;
    marker(&post, "old");
    assert_eq!(post.headers()["x-collection-action"], "PartialOld@show");
    assert_eq!(post.headers()["x-collection-methods"], "GET,POST,HEAD");
    assert_eq!(calls.old.load(Ordering::SeqCst), 1);
    let urls = send(&router, "localhost", "GET", "/urls").await?;
    assert_eq!(
        json_body(urls).await["data"],
        serde_json::json!({"split":"http://localhost/split","partial":"http://localhost/partial"})
    );
    Ok(())
}

#[tokio::test]
async fn collection_overwrite_keeps_position_and_early_dynamic_precedes_late_static() -> CaseResult
{
    let (router, calls) = build(|routes| {
        routes
            .group(
                RouteGroup::new().middleware(axum::middleware::from_fn(old_middleware)),
                |routes| routes.get_action("/items/{id}", "Old@show", old),
            )
            .get_action("/items/fixed", "Static@show", live)
            .get_action("/items/{id}", "Replacement@show", live)
            .get_action("/written/summary", "Summary@show", live)
            .get_action("/written/{id}", "Draft@show", live)
            .get_action("/aliases/{first}", "FirstAlias@show", live)
            .get_action("/aliases/{second}", "SecondAlias@show", old)
    })
    .await?;
    let dynamic = send(&router, "localhost", "GET", "/items/fixed").await?;
    assert_eq!(dynamic.headers()["x-collection-action"], "Replacement@show");
    assert_eq!(
        json_body(dynamic).await["data"]["parameters"],
        serde_json::json!([["id", "fixed"]])
    );
    let static_page = send(&router, "localhost", "GET", "/written/summary").await?;
    assert_eq!(static_page.headers()["x-collection-action"], "Summary@show");
    let alias = send(&router, "localhost", "GET", "/aliases/value").await?;
    assert_eq!(alias.headers()["x-collection-action"], "FirstAlias@show");
    assert_eq!(
        json_body(alias).await["data"]["parameters"],
        serde_json::json!([["first", "value"]])
    );
    no_old(&calls);
    Ok(())
}

#[tokio::test]
async fn collection_all_nonfallbacks_precede_domain_fallback_and_business_errors_stop() -> CaseResult
{
    let (router, calls) = build(|routes| {
        routes
            .group(RouteGroup::new().domain("a.test"), |routes| {
                routes.fallback(fallback)
            })
            .get_action("/explicit", "Explicit@show", live)
            .get_action("/business404", "Terminal@missing", business_404)
            .get_action("/business405", "Terminal@method", business_405)
            .post("/post", live)
    })
    .await?;
    marker(&send(&router, "a.test", "GET", "/explicit").await?, "live");
    for (path, status, action) in [
        ("/business404", StatusCode::NOT_FOUND, "Terminal@missing"),
        (
            "/business405",
            StatusCode::METHOD_NOT_ALLOWED,
            "Terminal@method",
        ),
    ] {
        let response = send(&router, "a.test", "GET", path).await?;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["x-collection-action"], action);
    }
    assert_eq!(calls.fallback.load(Ordering::SeqCst), 0);
    let missed = send(&router, "other.test", "GET", "/missing").await?;
    assert_eq!(missed.status(), StatusCode::NOT_FOUND);
    let post_only = send(&router, "other.test", "GET", "/post").await?;
    assert_eq!(post_only.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(post_only.headers()[header::ALLOW], "POST");
    marker(&send(&router, "a.test", "GET", "/post").await?, "fallback");
    assert_eq!(calls.fallback.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn collection_alternate_verbs_keep_fixed_order_query_and_custom_direct_only() -> CaseResult {
    let (router, _) = build(|routes| {
        routes
            .on(
                "/verbs",
                &[
                    "TRACE", "QUERY", "DELETE", "PATCH", "PUT", "POST", "HEAD", "GET",
                ],
                live,
            )
            .on("/custom", &["TRACE"], live)
            .on("/with-options", &["QUERY", "OPTIONS", "POST"], live)
    })
    .await?;
    let connect = send(&router, "localhost", "CONNECT", "/verbs").await?;
    assert_eq!(connect.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        connect.headers()[header::ALLOW],
        "GET, HEAD, POST, PUT, PATCH, DELETE, QUERY"
    );
    let options = send(&router, "localhost", "OPTIONS", "/verbs").await?;
    assert_eq!(options.status(), StatusCode::OK);
    assert_eq!(
        options.headers()[header::ALLOW],
        "GET,HEAD,POST,PUT,PATCH,DELETE,QUERY"
    );
    assert!(to_bytes(options.into_body(), 4096).await?.is_empty());
    marker(
        &send(&router, "localhost", "TRACE", "/custom").await?,
        "live",
    );
    for method in ["GET", "HEAD", "OPTIONS"] {
        let miss = send(&router, "localhost", method, "/custom").await?;
        assert_eq!(miss.status(), StatusCode::NOT_FOUND);
        assert!(!miss.headers().contains_key(header::ALLOW));
    }
    let explicit = send(&router, "localhost", "OPTIONS", "/with-options").await?;
    marker(&explicit, "live");
    assert_eq!(
        explicit.headers()["x-collection-methods"],
        "QUERY,OPTIONS,POST"
    );
    let get = send(&router, "localhost", "GET", "/with-options").await?;
    assert_eq!(get.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(get.headers()[header::ALLOW], "POST, OPTIONS, QUERY");
    Ok(())
}

const TCP_BUDGET: Duration = Duration::from_secs(5);

struct CollectionTcpServer {
    app: openrust::Application,
    address: std::net::SocketAddr,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for CollectionTcpServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
impl CollectionTcpServer {
    async fn finish(mut self) -> CaseResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mut task = self.task.take().ok_or("TCP server task missing")?;
        let joined = tokio::time::timeout(TCP_BUDGET, &mut task).await;
        if joined.is_err() {
            task.abort();
            let _ = tokio::time::timeout(TCP_BUDGET, &mut task).await;
        }
        let resources = self
            .app
            .shutdown(tokio::time::Instant::now() + TCP_BUDGET)
            .await;
        let released =
            tokio::time::timeout(TCP_BUDGET, tokio::net::TcpStream::connect(self.address)).await;
        joined???;
        resources?;
        assert!(
            released?.is_err(),
            "collection TCP listener {} remained open",
            self.address
        );
        Ok(())
    }
}

async fn tcp_response(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    host: &str,
    method: &str,
    path: &str,
) -> CaseResult<(StatusCode, axum::http::HeaderMap, Vec<u8>)> {
    let mut response = client
        .request(method.parse()?, format!("http://{address}{path}"))
        .header(header::HOST, host)
        .header(header::ACCEPT, "application/json")
        .header(header::CONNECTION, "close")
        .send()
        .await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 16 * 1024 {
            return Err("collection TCP response exceeded16KiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((status, headers, bytes))
}

async fn prepare_collection_tcp(
    app: &openrust::Application,
    calls: &Arc<Calls>,
) -> CaseResult<(axum::Router, tokio::net::TcpListener, std::net::SocketAddr)> {
    let setup: CaseResult<_> = async {
        let router = build_router(app)?.layer(Extension(Arc::clone(calls)));
        let listener =
            tokio::time::timeout(TCP_BUDGET, tokio::net::TcpListener::bind("127.0.0.1:0"))
                .await??;
        let address = listener.local_addr()?;
        Ok((router, listener, address))
    }
    .await;
    match setup {
        Ok(prepared) => Ok(prepared),
        Err(error) => {
            // listener 未移交时由局部所有权释放；App 的异步回调必须显式收尾。
            let cleanup = tokio::time::timeout(
                TCP_BUDGET,
                app.shutdown(tokio::time::Instant::now() + TCP_BUDGET),
            )
            .await;
            match cleanup {
                Ok(Ok(())) => Err(error),
                Ok(Err(cleanup_error)) => Err(std::io::Error::other(format!(
                    "collection TCP setup failed: {error}; App shutdown failed: {cleanup_error}"
                ))
                .into()),
                Err(cleanup_error) => Err(std::io::Error::other(format!(
                    "collection TCP setup failed: {error}; App shutdown timed out: {cleanup_error}"
                ))
                .into()),
            }
        }
    }
}

async fn assert_collection_over_tcp() -> CaseResult {
    use futures_util::FutureExt as _;
    // 复用公开 Provider/build_router/Axum listener 与仓库现有 stop+join+App shutdown 模式。
    let (app, calls) = build_app(|routes| {
        routes
            .group(
                RouteGroup::new()
                    .domain("a.test")
                    .middleware(axum::middleware::from_fn(old_middleware)),
                |routes| routes.get_action("/bc", "Old@show", old),
            )
            .group(RouteGroup::new().domain("a.testb"), |routes| {
                routes.get_action("/c", "New@show", live)
            })
            .group(
                RouteGroup::new().middleware(axum::middleware::from_fn(old_middleware)),
                |routes| routes.get("/a.testbc", old),
            )
            .on_action("/a.testbc", &["POST"], "Post@show", live)
    })
    .await?;
    let (router, listener, address) = prepare_collection_tcp(&app, &calls).await?;
    let (stop, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = signal.await;
            })
            .await
    });
    let server = CollectionTcpServer {
        app,
        address,
        stop: Some(stop),
        task: Some(task),
    };
    let outcome =
        std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(TCP_BUDGET)
                .redirect(reqwest::redirect::Policy::none())
                .pool_max_idle_per_host(0)
                .build()?;
            let (status, headers, _) =
                tcp_response(&client, address, "a.test", "GET", "/bc").await?;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert!(!headers.contains_key(header::ALLOW));
            let (status, headers, body) =
                tcp_response(&client, address, "a.testb", "GET", "/c").await?;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers["x-collection-action"], "New@show");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body)?["data"]["action"],
                "New@show"
            );
            for method in ["GET", "HEAD"] {
                let (status, headers, body) =
                    tcp_response(&client, address, "other.test", method, "/a.testbc").await?;
                assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
                assert_eq!(headers[header::ALLOW], "POST");
                if method == "HEAD" {
                    assert_eq!(body, [] as [u8; 0]);
                }
            }
            let (status, headers, body) =
                tcp_response(&client, address, "other.test", "OPTIONS", "/a.testbc").await?;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[header::ALLOW], "POST");
            assert_eq!(body, [] as [u8; 0]);
            let (status, headers, _) =
                tcp_response(&client, address, "other.test", "POST", "/a.testbc").await?;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers["x-collection-action"], "Post@show");
            no_old(&calls);
            Ok::<(), Box<dyn std::error::Error>>(())
        }))
        .catch_unwind()
        .await;
    // 即使断言 panic 或请求超时，也先停止并 join 自有 listener、关闭应用资源。
    let stopped = server.finish().await;
    match outcome {
        Ok(result) => {
            result??;
            stopped
        }
        Err(panic) => {
            let _ = stopped;
            std::panic::resume_unwind(panic)
        }
    }
}
