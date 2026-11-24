use std::{ffi::OsString, path::Path, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::{RouteGroup, RouteMetadata, Router, RoutingError},
};

fn report(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let text = format!(
        "{}:{}",
        request.route("seed").unwrap_or(""),
        request.route("tenant").unwrap_or("")
    );
    Box::pin(async move { Ok(Response::new(StatusCode::OK, Body::from(text))) })
}

fn method_report(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let methods = request
        .parts()
        .extensions
        .get::<RouteMetadata>()
        .unwrap()
        .methods
        .join(",");
    Box::pin(async move { Ok(Response::from(methods)) })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/", || "root")?;
    router.r#match(
        &[Method::POST, Method::GET, Method::POST],
        "/method-duplicates",
        method_report,
    )?;
    router.r#match(
        &[Method::HEAD, Method::GET, Method::HEAD],
        "/method-explicit-head",
        method_report,
    )?;
    let mut many = (0..12)
        .map(|i| Method::from_bytes(format!("EXTENDED-METHOD-{i}").as_bytes()).unwrap())
        .collect::<Vec<_>>();
    many.extend([Method::GET, Method::GET]);
    router.r#match(&many, "/method-many", method_report)?;
    router.r#match(
        &[Method::from_bytes(b"get").unwrap()],
        "/method-case",
        method_report,
    )?;

    router.get("/literal+(x)[y].txt", || "literal")?;
    router.get("/Case", || "case")?;
    router.get("/café/文件", || "unicode")?;
    router.get("/with space", || "space")?;
    router.get("/encoded%7B", || "encoded")?;
    router.get("/defaults", report)?.defaults("seed", "preset");
    router.get("/earlier/{value}", || "dynamic-first")?;
    router.get("/earlier/fixed", || "literal-later")?;
    router.get("/later/fixed", || "literal-first")?;
    router.get("/later/{value}", || "dynamic-later")?;
    router.get("/duplicate", || "old")?;
    router.get("/duplicate", || "replacement")?;
    router.get("/num/{id}", || "number")?.where_number("id")?;
    router.group(RouteGroup::new().domain("Static.Example.test"), |r| {
        r.get("/host", || "static-host")?;
        Ok(())
    })?;
    router.group(RouteGroup::new().domain("{tenant}.example.test"), |r| {
        r.get("/tenant", report)?.defaults("seed", "host-default");
        Ok(())
    })?;
    for length in [1024, 1025] {
        router.get(&format!("/{}", "z".repeat(length - 1)), || "boundary")?;
    }
    Ok(router)
}

fn application(root: &Path) -> Application {
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
async fn literal_paths_preserve_http_defaults_order_domains_and_cached_registration() {
    let root = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    for cached in [false, true] {
        if cached {
            assert_eq!(
                application(root.path())
                    .handle_command(&[OsString::from("route:cache")])
                    .await
                    .unwrap(),
                0
            );
        }
        let mut kernel = Kernel::new(application(root.path()));
        kernel.bootstrap().await.unwrap();
        assert_eq!(kernel.get_application().routes_are_cached(), cached);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .into_future(),
        );
        for (path, status, body) in [
            ("/", 200, "root"),
            ("/literal+(x)[y].txt", 200, "literal"),
            ("/literalXXtxt", 404, ""),
            ("/Case", 200, "case"),
            ("/case", 404, ""),
            ("/caf%C3%A9/%E6%96%87%E4%BB%B6", 200, "unicode"),
            ("/with%20space", 200, "space"),
            ("/encoded%257B", 200, "encoded"),
            ("/encoded%7B", 404, ""),
            ("/defaults", 200, "preset:"),
            ("/earlier/fixed", 200, "dynamic-first"),
            ("/later/fixed", 200, "literal-first"),
            ("/duplicate", 200, "replacement"),
            ("/num/123", 200, "number"),
            ("/num/nope", 404, ""),
        ] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(response.status(), status, "{path}, cached={cached}");
            if status == 200 {
                assert_eq!(response.text().await.unwrap(), body, "{path}");
            }
        }
        for (path, host, body) in [
            ("/host", "STATIC.example.TEST", "static-host"),
            ("/tenant", "acme.example.test", "host-default:acme"),
        ] {
            let response = client
                .get(format!("{base}{path}"))
                .header("host", host)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.text().await.unwrap(), body);
        }
        for (path, expected) in [
            ("/method-duplicates", "POST,GET,POST,HEAD".to_owned()),
            ("/method-explicit-head", "HEAD,GET,HEAD".to_owned()),
            (
                "/method-many",
                (0..12)
                    .map(|i| format!("EXTENDED-METHOD-{i}"))
                    .chain(["GET".into(), "GET".into(), "HEAD".into()])
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.text().await.unwrap(), expected);
        }
        for method in [Method::POST, Method::GET] {
            assert_eq!(
                client
                    .request(method, format!("{base}/method-duplicates"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                200
            );
        }
        let response = client
            .put(format!("{base}/method-duplicates"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 405);
        assert_eq!(response.headers()["allow"], "GET, HEAD, POST");
        let response = client
            .request(
                Method::from_bytes(b"EXTENDED-METHOD-11").unwrap(),
                format!("{base}/method-many"),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let response = client
            .request(
                Method::from_bytes(b"get").unwrap(),
                format!("{base}/method-case"),
            )
            .send()
            .await
            .unwrap();
        // 注册入口按既有契约归一化小写方法；传输层的小写方法本身不等于 GET。
        assert_eq!(response.status(), 405);
        assert_eq!(response.headers()["allow"], "GET, HEAD");
        assert_eq!(
            client
                .get(format!("{base}/method-case"))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        for length in [1024, 1025] {
            let response = client
                .get(format!("{base}/{}", "z".repeat(length - 1)))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.text().await.unwrap(), "boundary");
        }
        let head = client
            .head(format!("{base}/defaults"))
            .send()
            .await
            .unwrap();
        assert_eq!(head.status(), 200);
        assert!(head.bytes().await.unwrap().is_empty());
        let wrong = client
            .post(format!("{base}/defaults"))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), 405);
        assert_eq!(wrong.headers()["allow"], "GET, HEAD");
        server.abort();
        let _ = server.await;
    }
}

#[test]
fn literal_validation_and_long_template_limits_remain_atomic() {
    let mut router = Router::new();
    router.get("/kept", || "ok").unwrap();
    for uri in [
        "/bad}",
        "/bad?query",
        "/bad#fragment",
        "/bad\n",
        "/bad{",
        "/{bad name}",
    ] {
        assert!(
            matches!(
                router.get(uri, || "bad"),
                Err(RoutingError::InvalidUri { .. })
            ),
            "{uri:?}"
        );
        assert_eq!(router.get_routes().len(), 1);
    }
    for length in [1024, 1025, 4096, 512 * 1024] {
        let uri = format!("/{}", "a".repeat(length - 1));
        let expression = format!(r"\A{}\z", regex::escape(&uri));
        let expected = regex::RegexBuilder::new(&expression)
            .dot_matches_new_line(true)
            .build();
        let actual = router.get(&uri, || "long");
        assert_eq!(actual.is_ok(), expected.is_ok(), "length={length}");
        if expected.is_err() {
            assert!(matches!(
                actual,
                Err(RoutingError::InvalidRoutePattern { .. })
            ));
        }
    }
}
