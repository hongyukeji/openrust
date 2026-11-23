use std::{ffi::OsString, path::Path, sync::Arc};

use ::http::HeaderValue;
use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::{Kernel as _, Middleware},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HeaderMap, HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{RouteGroup, Router, RoutingError, ViewRouteOptions},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Route, View},
    },
    view::ViewData,
};

fn report(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let name = [
        "known",
        "root",
        "ordinary",
        "scoped-fallback",
        "root-fallback",
        "terminal",
    ]
    .into_iter()
    .find(|name| request.routeIs(&[*name]))
    .unwrap();
    let params = ["tenant", "fallbackPlaceholder", "value"]
        .into_iter()
        .filter_map(|key| {
            request
                .route(key)
                .map(|value| (key.to_owned(), serde_json::json!(value)))
        })
        .collect::<serde_json::Map<_, _>>();
    let value = serde_json::json!({"route":name,"parameters":params});
    let mut response = Response::json(StatusCode::OK, value.clone()).unwrap();
    response.parts_mut().headers.insert(
        "x-match",
        HeaderValue::from_str(&value.to_string()).unwrap(),
    );
    Box::pin(async move { Ok(response) })
}

struct Stamp;
impl Middleware for Stamp {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            Ok(next.run(request).await.header(
                "x-group".parse().unwrap(),
                HeaderValue::from_static("scoped"),
            ))
        })
    }
}

fn router_routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.group(
        RouteGroup::new()
            .prefix("scope")
            .domain("{tenant}.example.test")
            .middleware(Arc::new(Stamp)),
        |router| {
            assert!(router.fallback(report)?.name("scoped-fallback").isFallback);
            router.get("/known", report)?.name("known");
            router
                .view(
                    "/hello/{name?}",
                    "welcome",
                    ViewRouteOptions {
                        data: data(),
                        ..Default::default()
                    },
                )?
                .defaults("name", "Grace");
            Ok(())
        },
    )?;
    router.view("/router-view", "plain", Default::default())?;
    router.view(
        "/custom",
        "welcome",
        ViewRouteOptions {
            data: data(),
            status: StatusCode::CREATED,
            headers: HeaderMap::from_iter([(
                "x-probe".parse().unwrap(),
                HeaderValue::from_static("yes"),
            )]),
        },
    )?;
    router.get("/terminal", report)?.name("terminal").fallback();
    router
        .get("/{value}", report)?
        .name("ordinary")
        .r#where("value", "terminal")?;
    Ok(router)
}

fn data() -> ViewData {
    ViewData::from_iter([
        ("name".into(), "Taylor".into()),
        ("role".into(), "reader".into()),
    ])
}

fn facade_routes() -> Result<(), RoutingError> {
    Route::fallback(report)?;
    Route::fallbackWith(report, |route| {
        route.name("root-fallback");
        Ok(())
    })?;
    Route::getWith("/root", report, |route| {
        route.name("root");
        Ok(())
    })?;
    Route::view("/welcome", "plain", Default::default())?;
    Route::viewWith(
        "/welcome/{name}",
        "welcome",
        ViewRouteOptions {
            data: data(),
            ..Default::default()
        },
        |route| {
            route.name("welcome");
            Ok(())
        },
    )?;
    Route::view("/missing-template", "does-not-exist", Default::default())?;
    Route::view("/failure", "failure", Default::default())?;
    Route::group(RouteGroup::new().prefix("facade"), |router| {
        assert!(matches!(
            Route::fallback(report),
            Err(RoutingError::RegistrationBorrowed)
        ));
        assert!(matches!(
            Route::view("/borrowed", "plain", Default::default()),
            Err(RoutingError::RegistrationBorrowed)
        ));
        router.fallback(report)?.name("root-fallback");
        Ok(())
    })?;
    Ok(())
}

fn not_found(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Ok(Response::new(
            StatusCode::NOT_FOUND,
            Body::from("custom missing"),
        ))
    })
}
fn api_routes() -> Result<(), RoutingError> {
    Route::fallback(not_found)
}

struct Views;
impl ServiceProvider for Views {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            View::creator("welcome", |view| {
                // 创建器必须见到路径覆盖后的数据，不能在创建之后才用with补值。
                assert!(view.data().contains_key("name"));
                view.data_mut().insert("created".into(), true.into());
                Ok(())
            })?;
            View::composer("welcome", |view| {
                assert!(view.data().contains_key("created"));
                view.data_mut().insert("composed".into(), true.into());
                Ok(())
            })?;
            View::composer("failure", |_| {
                Err(openrust::view::ViewError::RenderFailed(
                    std::io::Error::other("视图组合器失败").into(),
                ))
            })?;
            Ok(())
        })
    }
}

fn application(root: &Path) -> Application {
    Application::configure(root.to_owned())
        .withProviders(vec![Box::new(Views)])
        .withRouting(
            Routing::new(router_routes)
                .web(facade_routes)
                .api(api_routes),
        )
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

#[tokio::test]
async fn public_shortcuts_preserve_matching_rendering_and_route_cache() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("reference/routing-shortcuts.json")).unwrap();
    let root = tempfile::tempdir().unwrap();
    let views = root.path().join("resources/views");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(views.join("plain.blade.html"), "Welcome").unwrap();
    std::fs::write(
        views.join("welcome.blade.html"),
        "{{ $name }}|{{ $role }}|{{ $created }}|{{ $composed }}",
    )
    .unwrap();
    std::fs::write(views.join("failure.blade.html"), "unreachable").unwrap();
    let client = reqwest::Client::new();
    for cached in [false, true] {
        if cached {
            assert_eq!(
                application(root.path())
                    .handleCommand(&[OsString::from("route:cache")])
                    .await
                    .unwrap(),
                0
            );
        }
        let mut kernel = Kernel::new(application(root.path()));
        kernel.bootstrap().await.unwrap();
        assert_eq!(kernel.get_application().routesAreCached(), cached);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        for case in golden[if cached { "cached_matches" } else { "matches" }]
            .as_array()
            .unwrap()
        {
            let method = case["method"].as_str().unwrap();
            let path = case["path"].as_str().unwrap();
            let response = client
                .request(method.parse().unwrap(), format!("{base}{path}"))
                .header("host", case["host"].as_str().unwrap())
                .send()
                .await
                .unwrap();
            if case["result"]["status"] == 405 {
                assert_eq!(response.status(), 405, "{case}");
                assert_eq!(
                    response.headers()["allow"],
                    case["result"]["headers"]["Allow"].as_str().unwrap()
                );
            } else if method == "OPTIONS" {
                assert_eq!(response.status(), 200);
                assert_eq!(response.headers()["allow"], "GET,HEAD");
            } else {
                assert_eq!(response.status(), 200, "{case}");
                let mut expected = case["result"].clone();
                if expected["parameters"] == serde_json::json!([]) {
                    expected["parameters"] = serde_json::json!({});
                }
                let actual: serde_json::Value =
                    serde_json::from_str(response.headers()["x-match"].to_str().unwrap()).unwrap();
                assert_eq!(actual, expected, "cached={cached}: {case}");
                if actual["route"] == "scoped-fallback" {
                    assert_eq!(response.headers()["x-group"], "scoped");
                }
            }
            if method == "HEAD" {
                assert!(response.bytes().await.unwrap().is_empty());
            }
        }
        for path in ["/welcome", "/router-view"] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(
                response.status(),
                golden["view"]["defaults"]["status"].as_u64().unwrap() as u16
            );
            assert_eq!(response.text().await.unwrap(), "Welcome");
        }
        let futures = ["Ada", "Lin"].map(|name| {
            let client = &client;
            let base = &base;
            async move {
                let response = client
                    .get(format!("{base}/welcome/{name}"))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                assert_eq!(response.text().await.unwrap(), format!("{name}|reader|1|1"));
            }
        });
        let [left, right] = futures;
        tokio::join!(left, right);
        let response = client.get(format!("{base}/custom")).send().await.unwrap();
        assert_eq!(
            response.status(),
            golden["view_custom"]["status"].as_u64().unwrap() as u16
        );
        assert_eq!(response.headers()["x-probe"], "yes");
        assert_eq!(response.text().await.unwrap(), "Taylor|reader|1|1");
        let response = client
            .get(format!("{base}/scope/hello"))
            .header("host", "one.example.test")
            .send()
            .await
            .unwrap();
        assert_eq!(response.headers()["x-group"], "scoped");
        assert_eq!(response.text().await.unwrap(), "Grace|reader|1|1");
        let response = client
            .head(format!("{base}/welcome/Ada"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(response.bytes().await.unwrap().is_empty());
        for path in ["/missing-template", "/failure"] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(response.status(), 500, "{path}");
        }
        assert_eq!(
            application(root.path())
                .handleCommand(&[OsString::from("view:cache")])
                .await
                .unwrap(),
            0
        );
        let response = client
            .get(format!("{base}/welcome/Ada"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "Ada|reader|1|1");

        let response = client
            .get(format!("{base}/api/unknown/nested"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
        assert_eq!(response.text().await.unwrap(), "custom missing");
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    assert_eq!(
        application(root.path())
            .handleCommand(&[OsString::from("route:clear")])
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn route_fallback_flag_can_be_reversed_without_reordering_registrations() {
    let mut router = Router::new();
    assert!(
        !router
            .get("/terminal", report)
            .unwrap()
            .name("terminal")
            .fallback()
            .setFallback(false)
            .isFallback
    );
    router.get("/{value}", report).unwrap().name("ordinary");
    let (parts, body) = ::http::Request::builder()
        .uri("/terminal")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = router
        .dispatch(&mut Request::from_parts(parts, body))
        .await
        .unwrap();
    let actual: serde_json::Value =
        serde_json::from_str(response.parts().headers["x-match"].to_str().unwrap()).unwrap();
    assert_eq!(actual["route"], "terminal");
    assert!(matches!(
        Route::fallback(report),
        Err(RoutingError::NoRegistrationScope)
    ));
    assert!(matches!(
        Route::view("/", "plain", Default::default()),
        Err(RoutingError::NoRegistrationScope)
    ));
}

#[tokio::test]
async fn registrar_contract_exposes_http_methods_and_normalizes_match() {
    use openrust::contracts::routing::Registrar;
    use openrust::http::Method;
    fn method(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        let value = HeaderValue::from_str(request.method().as_str()).unwrap();
        Box::pin(async {
            Ok(Response::empty(StatusCode::NO_CONTENT).header("x-method".parse().unwrap(), value))
        })
    }
    let mut router = Router::new();
    let registrar: &mut dyn Registrar = &mut router;
    registrar.get("/get", Arc::new(method)).unwrap();
    registrar.post("/post", Arc::new(method)).unwrap();
    registrar.put("/put", Arc::new(method)).unwrap();
    registrar.patch("/patch", Arc::new(method)).unwrap();
    registrar.delete("/delete", Arc::new(method)).unwrap();
    registrar.options("/options", Arc::new(method)).unwrap();
    registrar
        .r#match(
            &[Method::from_bytes(b"post").unwrap(), Method::GET],
            "/match",
            Arc::new(method),
        )
        .unwrap();
    assert!(matches!(
        registrar.r#match(&[], "/empty", Arc::new(method)),
        Err(RoutingError::MissingMethods)
    ));
    for (path, accepted) in [
        ("get", vec![Method::GET, Method::HEAD]),
        ("post", vec![Method::POST]),
        ("put", vec![Method::PUT]),
        ("patch", vec![Method::PATCH]),
        ("delete", vec![Method::DELETE]),
        ("options", vec![Method::OPTIONS]),
        ("match", vec![Method::POST, Method::GET, Method::HEAD]),
    ] {
        for method in accepted {
            let (parts, body) = ::http::Request::builder()
                .method(method.clone())
                .uri(format!("/{path}"))
                .body(Body::empty())
                .unwrap()
                .into_parts();
            let response = router
                .dispatch(&mut Request::from_parts(parts, body))
                .await
                .unwrap();
            assert_eq!(response.status(), 204);
            assert_eq!(response.headers()["x-method"], method.as_str());
        }
        let (parts, body) = ::http::Request::builder()
            .method(Method::CONNECT)
            .uri(format!("/{path}"))
            .body(Body::empty())
            .unwrap()
            .into_parts();
        assert!(matches!(
            router.dispatch(&mut Request::from_parts(parts, body)).await,
            Err(HttpError::MethodNotAllowed { .. })
        ));
    }
}
