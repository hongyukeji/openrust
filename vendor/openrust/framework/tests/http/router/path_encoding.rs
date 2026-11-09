//! 证明默认生产路由在业务分派、公开文件、fallback 和 CORS 前检查路径编码。

use super::{TestResult, TestRoutes, json_body};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    http::{StatusCode, header},
};
use openrust::{
    Application,
    config::FrameworkConfig,
    foundation::{ProviderBuilder, http::build_router},
    http::{HttpServiceProvider, middleware::ValidatePathEncoding},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tower::ServiceExt as _;

async fn router(public: &std::path::Path) -> openrust::Result<Router> {
    let mut config = FrameworkConfig::default();
    config.server.public_dir = public.to_owned();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://client.example".into()];
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/item/{value}", || async { "matched route" })
                .fallback(|| async { "fallback" })
        }));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let app = Application::from_provider_runtime(builder.start(sources).await?)?;
    build_router(&app)
}

#[tokio::test]
async fn malformed_path_rejected_before_route_fallback_static_and_cors() -> TestResult {
    let public = tempfile::tempdir()?;
    let assets = public.path().join("assets");
    std::fs::create_dir(&assets)?;
    std::fs::write(assets.join("asset.txt"), "static asset")?;
    let router = router(public.path()).await?;
    for (method, uri) in [
        ("GET", "/item/%FF"),
        ("GET", "/missing/%C0%AF"),
        ("GET", "/assets/%ED%A0%80.txt"),
        ("OPTIONS", "/item/%E4%BD"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::ORIGIN, "https://client.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .header("x-request-id", "path-encoding-fixture")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method} {uri}");
        assert!(
            !response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
        assert_eq!(response.headers()["x-request-id"], "path-encoding-fixture");
        let body = json_body(response).await;
        assert_eq!(body["code"], "http_request_path_encoding_invalid");
        assert_eq!(body["message"], "Malformed URL.");
    }

    for (uri, expected) in [
        ("/item/%E4%BD%A0", "matched route"),
        ("/item/%ZZ", "matched route"),
        ("/item/%25FF", "matched route"),
        ("/item/value?query=%FF", "matched route"),
        ("/missing/valid", "fallback"),
        ("/assets/asset.txt", "static asset"),
    ] {
        let response = router
            .clone()
            .oneshot(Request::get(uri).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(to_bytes(response.into_body(), 4096).await?, expected);
    }

    let response = router
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/item/valid")
                .header(header::ORIGIN, "https://client.example")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())?,
        )
        .await?;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://client.example"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_percent_decoded_path_is_http_400_over_tcp() -> TestResult {
    let public = tempfile::tempdir()?;
    let router = router(public.path()).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        let mut socket = tokio::net::TcpStream::connect(address).await?;
        socket
            .write_all(b"GET /missing/%FF HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut response = String::new();
        socket.read_to_string(&mut response).await?;
        Ok::<_, std::io::Error>(response)
    })
    .await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    let response = outcome??;
    assert!(response.starts_with("HTTP/1.1 400 "));
    assert!(response.contains(r#""code":"http_request_path_encoding_invalid""#));
    Ok(())
}

#[test]
fn path_encoding_validator_preserves_literal_and_one_pass_escape_semantics() -> TestResult {
    for path in ["/%ZZ", "/incomplete%", "/%25FF", "/%E4%BD%A0+"] {
        ValidatePathEncoding::validate(path)?;
    }
    let Err(error) = ValidatePathEncoding::validate("/%FF") else {
        return Err("非法 UTF-8 应返回错误".into());
    };
    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error.code(), "http_request_path_encoding_invalid");
    Ok(())
}
