use std::{collections::BTreeMap, ffi::OsString, path::Path, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::{Kernel as _, Middleware},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{RouteMetadata, Router, RoutingError},
    support::facades::{Config, Facade, Route},
};

#[derive(Clone, Default)]
struct Trace(Vec<&'static str>);

struct Stamp;
impl Middleware for Stamp {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            request
                .parts_mut()
                .extensions
                .get_or_insert_default::<Trace>()
                .0
                .push("before");
            let response = next.run(request).await;
            Ok(response.header("x-target".parse().unwrap(), "ran".parse().unwrap()))
        })
    }
}

fn report(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        // 让并发请求交错，不能依靠碰巧串行来证明请求隔离。
        tokio::task::yield_now().await;
        let named = if request.routeIs(&["named-fallback"]) {
            "named-fallback"
        } else {
            "target"
        };
        let parameters: serde_json::Map<String, serde_json::Value> =
            ["old", "value", "fallbackPlaceholder"]
                .into_iter()
                .filter_map(|key| request.route(key).map(|value| (key.into(), value.into())))
                .collect();
        let trace = request
            .parts()
            .extensions
            .get::<Trace>()
            .map(|trace| trace.0.clone())
            .unwrap_or_default();
        use openrust::contracts::{container::Container as _, foundation::Application as _};
        let current = openrust::support::facades::App::getFacadeRoot()?;
        assert!(Arc::ptr_eq(
            &Route::getFacadeRoot()?,
            &current.container().make::<Router>()?
        ));
        let app = Config::get("app.name")?.unwrap();
        let metadata = request.parts().extensions.get::<RouteMetadata>().unwrap();
        assert!(!metadata.uri.contains("to-"));
        Ok(Response::json(
            StatusCode::OK,
            serde_json::json!({"parameters":parameters,"named":named,"trace":trace}),
        )
        .unwrap()
        .header(
            "x-app".parse().unwrap(),
            app.as_str().unwrap().parse().unwrap(),
        ))
    })
}

fn to_fallback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respondWithRoute("named-fallback", request))
}
fn to_target(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respondWithRoute("target", request))
}
fn unknown(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respondWithRoute("absent", request))
}
fn failure(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Err(HttpError::AuthorizationDenied("拒绝访问".into())) })
}
fn to_failure(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respondWithRoute("failure", request))
}
fn pending(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(std::future::pending())
}
fn to_pending(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respondWithRoute("pending", request))
}
fn nested(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respondWithRoute("intermediate", request))
}
fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router
        .fallback(report)?
        .name("named-fallback")
        .middleware(Arc::new(Stamp));
    router
        .get("/target/{value?}", report)?
        .name("target")
        .defaults("value", "default")
        .middleware(Arc::new(Stamp));
    router.get("/to-fallback/{old}", to_fallback)?;
    router.get("/to-target/{old}", to_target)?;
    router.get("/unknown", unknown)?;
    router.get("/denied", failure)?.name("failure");
    router.get("/to-failure", to_failure)?;
    router.get("/pending", pending)?.name("pending");
    router.get("/to-pending", to_pending)?;
    router.get("/intermediate", to_target)?.name("intermediate");
    router.get("/nested", nested)?;
    Ok(router)
}

fn application(root: &Path) -> Application {
    Application::configure(root.to_owned())
        .withConfig(|base, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                serde_json::json!({"name":base.file_name().unwrap().to_str().unwrap()}),
            )]))
        })
        .withRouting(Routing::new(routes))
        .create()
        .unwrap()
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
async fn server(root: &Path) -> (String, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(application(root));
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        base,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

#[tokio::test]
async fn named_routes_use_default_runtime_binding_and_preserve_cache_and_application_isolation() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("reference/routing-named.json")).unwrap();
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let client = reqwest::Client::new();
    for cached in [false, true] {
        for root in &roots {
            if cached {
                assert_eq!(
                    application(root.path())
                        .handleCommand(&[OsString::from("route:cache")])
                        .await
                        .unwrap(),
                    0
                );
            }
        }
        let (left, left_task) = server(roots[0].path()).await;
        let (right, right_task) = server(roots[1].path()).await;
        for (path, expected) in golden.as_object().unwrap() {
            let (left_response, right_response) = tokio::join!(
                client.get(format!("{left}{path}")).send(),
                client.get(format!("{right}{path}")).send()
            );
            for (response, root) in [
                (left_response.unwrap(), &roots[0]),
                (right_response.unwrap(), &roots[1]),
            ] {
                assert_eq!(
                    response.status().as_u16(),
                    expected["status"].as_u64().unwrap() as u16,
                    "{path}"
                );
                if expected.get("body").is_some() {
                    assert_eq!(response.headers()["x-target"], "ran");
                    assert_eq!(
                        response.headers()["x-app"],
                        root.path().file_name().unwrap().to_str().unwrap()
                    );
                    assert_eq!(
                        response.json::<serde_json::Value>().await.unwrap(),
                        expected["body"],
                        "{path}"
                    );
                }
            }
        }
        assert_eq!(
            client
                .get(format!("{left}/to-failure"))
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        let response = client.get(format!("{right}/nested")).send().await.unwrap();
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["parameters"],
            serde_json::json!({"value":"default"})
        );
        let response = client
            .head(format!("{left}/to-fallback/head"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-target"], "ran");
        assert!(response.bytes().await.unwrap().is_empty());
        left_task.abort();
        right_task.abort();
        let _ = tokio::join!(left_task, right_task);
    }
    assert!(Route::getFacadeRoot().is_err());
    let mut outside = request("/target/outside");
    assert!(
        Route::respondWithRoute("target", &mut outside)
            .await
            .is_err()
    );
}

fn request(path: &str) -> Request {
    let (parts, body) = ::http::Request::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap()
        .into_parts();
    Request::from_parts(parts, body)
}

#[tokio::test]
async fn router_named_binding_does_not_require_matching_uri_or_method_and_handles_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let mut router = routes(&application(root.path())).unwrap();
    router
        .get("/constrained/{value?}", failure)
        .unwrap()
        .name("constrained")
        .defaults("value", "default")
        .where_number("value")
        .unwrap();
    let mut unrelated = request("/different");
    unrelated.parts_mut().method = ::http::Method::POST;
    let response = router
        .respondWithRoute("constrained", &mut unrelated)
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert_eq!(unrelated.route("value"), Some("default"));
    assert!(unrelated.routeIs(&["constrained"]));
    assert!(matches!(
        router.respondWithRoute("absent", &mut unrelated).await,
        Err(HttpError::Internal { .. })
    ));
    // 缺少名字不能先破坏原有路由状态。
    assert!(unrelated.routeIs(&["constrained"]));
    let mut suspended = request("/original");
    let mut future = Box::pin(router.respondWithRoute("pending", &mut suspended));
    assert!(futures_util::poll!(&mut future).is_pending());
    drop(future);
    assert!(suspended.routeIs(&["pending"]));
    let response = router
        .respondWithRoute("failure", &mut suspended)
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert!(suspended.routeIs(&["failure"]));
    assert_eq!(suspended.route("old"), None);
}

#[derive(Default)]
struct Lifecycle(std::sync::Mutex<Vec<&'static str>>);
struct LifecycleProvider(Arc<Lifecycle>);
impl openrust::support::ServiceProvider for LifecycleProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            use openrust::contracts::{container::Container as _, foundation::Application as _};
            app.container().instance(self.0.clone())?;
            Ok(())
        })
    }
}
struct Probe {
    lifecycle: Arc<Lifecycle>,
    target: bool,
}
impl Middleware for Probe {
    fn terminable(&self) -> Option<&dyn openrust::contracts::http::TerminableMiddleware> {
        Some(self)
    }
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            self.lifecycle.0.lock().unwrap().push(if self.target {
                "target.before"
            } else {
                "outer.before"
            });
            let response = next.run(request).await;
            assert!(request.routeIs(&["target"]));
            assert_eq!(request.route("old"), None);
            self.lifecycle.0.lock().unwrap().push(if self.target {
                "target.after"
            } else {
                "outer.after"
            });
            Ok(response)
        })
    }
}
impl openrust::contracts::http::TerminableMiddleware for Probe {
    fn terminate<'a>(
        &'a self,
        request: &'a openrust::http::RequestParts,
        _: &'a openrust::http::ResponseParts,
    ) -> HttpFuture<'a, ()> {
        Box::pin(async move {
            assert_eq!(
                request.extensions.get::<RouteMetadata>().unwrap().uri,
                "/target"
            );
            self.lifecycle.0.lock().unwrap().push(if self.target {
                "target.terminate"
            } else {
                "outer.terminate"
            });
        })
    }
}
fn lifecycle_routes(app: &Application) -> Result<Router, RoutingError> {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let lifecycle = app.container().make::<Lifecycle>()?;
    let mut router = Router::new();
    router
        .get("/entry/{old}", to_target)?
        .middleware(Arc::new(Probe {
            lifecycle: lifecycle.clone(),
            target: false,
        }));
    router
        .get("/target", failure)?
        .name("target")
        .middleware(Arc::new(Probe {
            lifecycle,
            target: true,
        }));
    router.get("/pending", pending)?.name("pending");
    router.get("/to-pending", to_pending)?;
    Ok(router)
}
#[tokio::test]
async fn named_dispatch_replaces_terminators_and_cancellation_releases_runtime_scope() {
    let root = tempfile::tempdir().unwrap();
    let lifecycle = Arc::new(Lifecycle::default());
    let app = Application::configure(root.path().to_owned())
        .withProviders(vec![Box::new(LifecycleProvider(lifecycle.clone()))])
        .withRouting(Routing::new(lifecycle_routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let mut suspended = request("/to-pending");
    let mut future = Box::pin(kernel.handle(&mut suspended));
    assert!(futures_util::poll!(&mut future).is_pending());
    drop(future);
    assert!(Route::getFacadeRoot().is_err());
    let mut entry = request("/entry/original");
    let response = kernel.handle(&mut entry).await;
    assert_eq!(response.status(), 403);
    kernel.terminate(entry.parts(), response.parts()).await;
    assert_eq!(
        *lifecycle.0.lock().unwrap(),
        [
            "outer.before",
            "target.before",
            "target.after",
            "outer.after",
            "target.terminate"
        ]
    );
    assert!(Route::getFacadeRoot().is_err());
}
