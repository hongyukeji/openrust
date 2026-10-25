//! End-to-end behaviour of the HTTP layer through `tower::ServiceExt::oneshot`.

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use openrust_core::config::FrameworkConfig;
use openrust_core::{AppBuilder, finalize};
use openrust_http::axum::Json;
use openrust_http::{AppBuilderHttpExt, HttpPlugin, Inject, Validated, build_router};
use serde::Deserialize;
use tower::ServiceExt;
use validator::Validate;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Greeting(&'static str);

#[derive(Deserialize, Validate)]
struct CreatePost {
    #[validate(length(min = 3, message = "title must be at least 3 characters"))]
    title: String,
    #[validate(range(min = 1))]
    pages: u32,
}

async fn greet(Inject(greeting): Inject<Greeting>) -> &'static str {
    greeting.0
}

async fn create(Validated(Json(input)): Validated<Json<CreatePost>>) -> Json<String> {
    Json(input.title)
}

async fn missing(Inject(_greeting): Inject<String>) -> &'static str {
    "unreachable"
}

// The panic is the behaviour under test: it must become a 500 problem, not kill the server.
#[allow(clippy::panic)]
async fn boom() -> &'static str {
    panic!("handler exploded")
}

async fn router() -> openrust_core::Result<axum::Router> {
    router_with_config(FrameworkConfig::default()).await
}

async fn router_with_config(config: FrameworkConfig) -> openrust_core::Result<axum::Router> {
    let mut builder = AppBuilder::new(config, figment::Figment::new());
    builder
        .plugin(HttpPlugin)
        .insert(Greeting("hello"))
        .routes(|r| {
            r.nest("/api", |api| {
                api.get("/greet", greet)
                    .post("/posts", create)
                    .get("/missing", missing)
                    .get("/boom", boom)
                    .get("/large", large)
                    .get("/slow", slow)
            })
        });
    let app = finalize(builder).await?;
    build_router(&app)
}

async fn large() -> String {
    "compressible response ".repeat(100)
}

async fn slow() -> &'static str {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    "late"
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
    assert_eq!(json_body(response).await["status"], "ok");
    Ok(())
}

#[tokio::test]
async fn inject_resolves_registered_services() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/greet").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    assert_eq!(&bytes[..], b"hello");
    Ok(())
}

#[tokio::test]
async fn inject_of_unregistered_service_is_a_problem_not_a_panic() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/missing").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = json_body(response).await;
    assert_eq!(body["code"], "internal_error");
    assert!(body.get("detail").is_none(), "internal details are hidden");
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
        Some("application/problem+json")
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
    assert_eq!(json_body(response).await, "Hello");

    let malformed = Request::post("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))?;
    let response = router().await?.oneshot(malformed).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["code"], "bad_request");
    Ok(())
}

#[tokio::test]
async fn panics_become_500_problems() -> TestResult {
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
            assert_eq!(bytes.len(), 2200);
        }
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
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"https://example.com".parse()?)
    );
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
