//! 同一公开契约在正式 Router 和隔离原型上运行；没有实验性公开入口。
#[path = "prototypes/workload.rs"]
mod workload;
use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::Request,
    routing::{RouteGroup, Router, RoutingError},
};
use std::{ffi::OsString, path::Path, sync::Arc};
fn routes(app: &Application) -> Result<Router, RoutingError> {
    let base = app.base_path();
    let (shape, count) = base
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .split_once('-')
        .unwrap();
    let count = count.parse().unwrap();
    if shape == "edge" {
        return edge_routes(count == 1);
    }
    let mut router = Router::new();
    for index in 0..count {
        router.get(&workload::path(shape, index), move || {
            format!("route:{index}")
        })?;
    }
    Ok(router)
}
fn app(root: &Path) -> Application {
    Application::configure(root.to_path_buf())
        .with_routing(Routing::new(routes))
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
async fn all_sizes_and_shapes_preserve_native_http_and_artisan_cache() {
    let directory = tempfile::tempdir().unwrap();
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    for shape in ["static", "dynamic", "overlap"] {
        for count in [128, 1000, 10000] {
            let root = directory.path().join(format!("{shape}-{count}"));
            std::fs::create_dir_all(&root).unwrap();
            let cache = root.join("storage/framework/cache/routes.json");
            for cached in [false, true] {
                if cached {
                    assert_eq!(
                        app(&root)
                            .handle_command(&[OsString::from("route:cache")])
                            .await
                            .unwrap(),
                        0
                    );
                    let bytes = std::fs::read(&cache).unwrap();
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["version"],
                        1
                    );
                    assert_eq!(
                        app(&root)
                            .handle_command(&[OsString::from("route:cache")])
                            .await
                            .unwrap(),
                        0
                    );
                    assert_eq!(std::fs::read(&cache).unwrap(), bytes);
                    if let Some(output) = std::env::var_os("ROUTE_PROTOTYPE_CACHE_OUTPUT") {
                        let output = std::path::PathBuf::from(output);
                        let workflow = Path::new(env!("CARGO_MANIFEST_DIR"))
                            .ancestors()
                            .nth(3)
                            .unwrap()
                            .parent()
                            .unwrap();
                        assert!(
                            output.starts_with(workflow),
                            "缓存对照输出必须位于隔离工作流内"
                        );
                        std::fs::create_dir_all(&output).unwrap();
                        std::fs::write(output.join(format!("{shape}-{count}.json")), &bytes)
                            .unwrap();
                    }
                }
                let mut kernel = Kernel::new(app(&root));
                kernel.bootstrap().await.unwrap();
                assert_eq!(kernel.get_application().routes_are_cached(), cached);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let base = format!("http://{}", listener.local_addr().unwrap());
                let service = transport.with_state(Arc::new(kernel)).into_make_service();
                let server =
                    tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
                for (path, expected) in workload::queries(shape, count) {
                    let response = client.get(format!("{base}{path}")).send().await.unwrap();
                    assert_eq!(
                        response.status(),
                        if expected.is_some() { 200 } else { 404 },
                        "{shape} {count} {cached} {path}"
                    );
                    if let Some(index) = expected {
                        assert_eq!(response.text().await.unwrap(), format!("route:{index}"));
                    }
                }
                let first = &workload::queries(shape, count)[0].0;
                let response = client.head(format!("{base}{first}")).send().await.unwrap();
                assert_eq!(response.status(), 200);
                assert!(response.bytes().await.unwrap().is_empty());
                let response = client.put(format!("{base}{first}")).send().await.unwrap();
                assert_eq!(response.status(), 405);
                assert_eq!(response.headers()["allow"], "GET, HEAD");
                server.abort();
                let _ = server.await;
            }
            assert_eq!(
                app(&root)
                    .handle_command(&[OsString::from("route:clear")])
                    .await
                    .unwrap(),
                0
            );
            assert!(!cache.exists());
            println!("HTTP／Artisan 矩阵通过：{shape} {count}");
        }
    }
}

fn edge_routes(reverse: bool) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/sentinel", || "sentinel")?;
    let mut paths = [("/a/b/{id}", "longer"), ("/a/{rest}", "shorter")];
    if reverse {
        paths.reverse();
    }
    for (uri, value) in paths {
        let route = router.get(uri, move || value)?;
        if uri == "/a/{rest}" {
            route.r#where("rest", ".*")?;
        }
    }
    router.get("/file.{ext?}", || "optional")?;
    router.get("/xy/{id}", || "ascii")?;
    router.get("/café/{id}", || "unicode")?;
    router.get("/duplicate", || "old")?;
    router.get("/duplicate", || "new")?;
    router.group(RouteGroup::new().domain("one.test"), |r| {
        r.get("/hosts/{id}", || "host-one")?;
        Ok(())
    })?;
    router.group(RouteGroup::new().domain("two.test"), |r| {
        r.get("/hosts/fixed", || "host-two")?;
        Ok(())
    })?;
    let fallback = router.get("/{rest}", || "fallback")?;
    fallback.r#where("rest", ".*")?;
    fallback.is_fallback = true;
    Ok(router)
}
#[tokio::test]
async fn prefix_buckets_preserve_order_and_boundaries_over_http_and_artisan() {
    let directory = tempfile::tempdir().unwrap();
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    for reverse in [false, true] {
        let root = directory
            .path()
            .join(format!("edge-{}", usize::from(reverse)));
        std::fs::create_dir_all(&root).unwrap();
        let cache = root.join("storage/framework/cache/routes.json");
        for cached in [false, true] {
            if cached {
                for _ in 0..2 {
                    assert_eq!(
                        app(&root)
                            .handle_command(&[OsString::from("route:cache")])
                            .await
                            .unwrap(),
                        0
                    );
                }
            }
            let mut kernel = Kernel::new(app(&root));
            kernel.bootstrap().await.unwrap();
            assert_eq!(kernel.get_application().routes_are_cached(), cached);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let service = transport.with_state(Arc::new(kernel)).into_make_service();
            let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
            for (path, host, body) in [
                (
                    "/a/b/fixed",
                    "localhost",
                    if reverse { "shorter" } else { "longer" },
                ),
                ("/file", "localhost", "optional"),
                ("/%E6%96%87/a", "localhost", "fallback"),
                ("/caf%C3%A9/42", "localhost", "unicode"),
                ("/duplicate", "localhost", "new"),
                ("/missing/path", "localhost", "fallback"),
                ("/hosts/fixed", "ONE.TEST", "host-one"),
                ("/hosts/fixed", "TWO.TEST", "host-two"),
            ] {
                let response = client
                    .get(format!("{base}{path}"))
                    .header("host", host)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200, "{reverse} {cached} {path} {host}");
                assert_eq!(response.text().await.unwrap(), body);
            }
            let response = client
                .head(format!("{base}/a/b/fixed"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert!(response.bytes().await.unwrap().is_empty());
            let response = client
                .put(format!("{base}/a/b/fixed"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 405);
            assert_eq!(response.headers()["allow"], "GET, HEAD");
            server.abort();
            let _ = server.await;
        }
        assert_eq!(
            app(&root)
                .handle_command(&[OsString::from("route:clear")])
                .await
                .unwrap(),
            0
        );
        assert!(!cache.exists());
    }
}
