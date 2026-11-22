use std::{ffi::OsString, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::{RouteGroup, Router, RoutingError},
    support::facades::Route,
};

fn echo(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let body = format!(
        "{}:{}",
        request.parts().method,
        request.route("id").unwrap_or("")
    );
    Box::pin(async move { Ok(Response::new(StatusCode::OK, Body::from(body))) })
}

fn first(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::new(StatusCode::OK, Body::from("first"))) })
}

fn last(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::new(StatusCode::OK, Body::from("last"))) })
}

fn router_routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.post("/router/post/{id}", echo)?.name("create");
    router.put("/router/put/{id}", echo)?;
    router.patch("/router/patch/{id}", echo)?;
    router.delete("/router/delete/{id}", echo)?;
    router.options("/router/options/{id}", echo)?;
    router.query("/router/query/{id}", echo)?;
    router.get("/router/get/{id}", echo)?;
    router.r#match(
        &[Method::from_bytes(b"post").unwrap()],
        "/router/lower/{id}",
        echo,
    )?;
    router.any("/router/any/{id}", echo)?;
    Ok(router)
}

fn facade_routes() -> Result<(), RoutingError> {
    let line = line!() + 1;
    Route::post("/post/{id}", echo)?;
    Route::put("/put/{id}", echo)?;
    Route::patch("/patch/{id}", echo)?;
    Route::delete("/delete/{id}", echo)?;
    Route::options("/options/{id}", echo)?;
    Route::query("/query/{id}", echo)?;
    Route::get("/get/{id}", echo)?;
    Route::any("/any/{id}", echo)?;
    Route::r#match(
        &[Method::from_bytes(b"post").unwrap(), Method::GET],
        "/match/{id}",
        echo,
    )?;
    Route::any("/any-first", first)?;
    Route::post("/any-first", last)?;
    Route::post("/any-last", first)?;
    Route::any("/any-last", last)?;
    Route::r#match(&[Method::GET, Method::POST], "/match-first", first)?;
    Route::post("/match-first", last)?;
    Route::post("/match-last", first)?;
    Route::r#match(&[Method::GET, Method::POST], "/match-last", last)?;
    Route::r#match(&[Method::HEAD], "/head-first", first)?;
    Route::get("/head-first", last)?;
    Route::get("/head-last", first)?;
    Route::r#match(&[Method::HEAD], "/head-last", last)?;
    Route::any("/{id}/overlap", first)?;
    Route::post("/literal/overlap", last)?;
    Route::group(RouteGroup::new(), |router| {
        let info = router
            .getRoutes()
            .into_iter()
            .find(|r| r.uri == "api/post/{id}")
            .unwrap();
        assert_eq!(info.path, format!("{}:{line}", file!()));
        assert!(info.action.ends_with("::echo"));
        assert!(matches!(
            Route::post("/borrowed", echo),
            Err(RoutingError::RegistrationBorrowed)
        ));
        Ok(())
    })?;
    assert!(matches!(
        Route::r#match(&[], "/empty", echo),
        Err(RoutingError::MissingMethods)
    ));
    Ok(())
}

fn application(root: &std::path::Path) -> Application {
    Application::configure(root.to_owned())
        .withRouting(Routing::new(router_routes).api(facade_routes))
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
async fn method_dispatch_and_registration_order_survive_route_cache() {
    let root = tempfile::tempdir().unwrap();
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
        // CONNECT 的网络请求目标使用 authority；在 Kernel 边界核对路径路由契约。
        for prefix in ["router", "api"] {
            let (parts, body) = axum::http::Request::builder()
                .method(Method::CONNECT)
                .uri(format!("/{prefix}/any/42"))
                .body(Body::empty())
                .unwrap()
                .into_parts();
            let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
            assert_eq!(
                response.parts().headers["allow"],
                "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS, QUERY"
            );
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        let response = client
            .post(format!("{base}/router/lower/42"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), "POST:42");
        for prefix in ["router", "api"] {
            for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY"] {
                let url = format!("{base}/{prefix}/{}/42", method.to_ascii_lowercase());
                let response = client
                    .request(Method::from_bytes(method.as_bytes()).unwrap(), &url)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200, "{url}");
                assert_eq!(response.text().await.unwrap(), format!("{method}:42"));
                let response = client.get(&url).send().await.unwrap();
                if method != "GET" {
                    assert_eq!(response.status(), 405, "{url}");
                    assert_eq!(response.headers()["allow"], method);
                }
            }
            for method in [
                "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY", "TRACE",
            ] {
                let response = client
                    .request(
                        Method::from_bytes(method.as_bytes()).unwrap(),
                        format!("{base}/{prefix}/any/42"),
                    )
                    .send()
                    .await
                    .unwrap();
                if method == "TRACE" {
                    assert_eq!(response.status(), 405);
                    assert_eq!(
                        response.headers()["allow"],
                        "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS, QUERY"
                    );
                } else {
                    assert_eq!(response.status(), 200);
                    assert_eq!(
                        response.text().await.unwrap(),
                        if method == "HEAD" {
                            String::new()
                        } else {
                            format!("{method}:42")
                        }
                    );
                }
            }
            let head = client
                .head(format!("{base}/{prefix}/get/42"))
                .send()
                .await
                .unwrap();
            assert_eq!(head.status(), 200);
            assert_eq!(head.headers()["content-length"], "7");
            assert!(head.bytes().await.unwrap().is_empty());
        }
        for (method, path, expected) in [
            ("POST", "any-first", "last"),
            ("GET", "any-first", "first"),
            ("POST", "any-last", "last"),
            ("GET", "any-last", "last"),
            ("POST", "match-first", "last"),
            ("GET", "match-first", "first"),
            ("POST", "match-last", "last"),
            ("GET", "match-last", "last"),
            ("POST", "literal/overlap", "first"),
            ("GET", "match/42", "GET:42"),
            ("POST", "match/42", "POST:42"),
        ] {
            let response = client
                .request(
                    Method::from_bytes(method.as_bytes()).unwrap(),
                    format!("{base}/api/{path}"),
                )
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "{method} {path}");
            assert_eq!(response.text().await.unwrap(), expected, "{method} {path}");
        }
        for path in ["match/42", "head-first", "head-last"] {
            let response = client
                .head(format!("{base}/api/{path}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(
                response.headers()["content-length"],
                if path == "match/42" { "7" } else { "4" }
            );
            assert!(response.bytes().await.unwrap().is_empty());
        }
        let response = client
            .patch(format!("{base}/api/match/42"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 405);
        assert_eq!(response.headers()["allow"], "GET, HEAD, POST");
        let response = client
            .request(Method::OPTIONS, format!("{base}/api/match/42"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["allow"], "GET,HEAD,POST");
        server.abort();
    }
}

#[test]
fn dedicated_methods_preserve_registration_errors_and_caller() {
    assert!(matches!(
        Route::post("/x", echo),
        Err(RoutingError::NoRegistrationScope)
    ));
    let mut router = Router::new();
    assert!(matches!(
        router.post("/{invalid", echo),
        Err(RoutingError::InvalidUri { .. })
    ));
    assert!(matches!(
        router.r#match(&[], "/x", echo),
        Err(RoutingError::MissingMethods)
    ));
    let line = line!() + 1;
    router.post("/x", echo).unwrap().name("create");
    let info = &router.getRoutes()[0];
    assert_eq!(info.path, format!("{}:{line}", file!()));
    assert_eq!(info.name.as_deref(), Some("create"));
    assert!(info.action.ends_with("::echo"));
    assert_eq!(info.method, "POST");
}
