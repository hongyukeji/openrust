//! End-to-end behaviour of the HTTP layer through `tower::ServiceExt::oneshot`.

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use openrust_core::config::FrameworkConfig;
use openrust_core::{AppBuilder, finalize};
use openrust_http::axum::Json;
use openrust_http::{AppBuilderHttpExt, HttpProvider, Inject, Validated, build_router};
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

async fn store(Validated(Json(input)): Validated<Json<CreatePost>>) -> Json<String> {
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
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder
        .provider(HttpProvider)
        .provide(Greeting("hello"))
        .routes(|r| {
            r.group("/api", |api| {
                api.get("/greet", greet)
                    .post("/posts", store)
                    .get("/missing", missing)
                    .get("/boom", boom)
            })
        });
    let app = finalize(builder).await?;
    Ok(build_router(&app))
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tokio::test]
async fn health_and_request_id_are_installed_by_the_provider() -> TestResult {
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
