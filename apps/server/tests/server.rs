//! Integration boundaries of the assembled server.

#[path = "../src/app.rs"]
mod app;
#[path = "../src/routes.rs"]
mod routes;

use openrust::ServiceExt;
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::build_router;
use openrust::{config, lifecycle};

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn router() -> Result<openrust::http::axum::Router, openrust::Error> {
    let app = lifecycle::bootstrap::<app::Server>().await?;
    build_router(&app)
}

#[test]
fn health_has_request_id() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let response = router()
            .await?
            .oneshot(Request::get("/health").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
        assert!(
            std::str::from_utf8(&to_bytes(response.into_body(), usize::MAX).await?)?.contains("ok")
        );
        Ok(())
    })
}

#[test]
fn root_reports_server_name_and_version() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let response = router()
            .await?
            .oneshot(Request::get("/").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let text = std::str::from_utf8(&body)?;
        assert!(text.contains("\"name\":\"openrust-server\""), "{text}");
        assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
        Ok(())
    })
}

#[test]
fn config_show_runs_and_redacts_secrets() -> TestResult {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_openrust-server"))
        .args(["config", "show"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("OPENRUST_API__TOKEN", "server-secret-value")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("openrust-server"));
    assert!(stdout.contains("********"), "{stdout}");
    assert!(!stdout.contains("server-secret-value"));
    Ok(())
}

#[test]
fn default_config_loads_from_package_directory() -> TestResult {
    let loader = config::ConfigLoader::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/config"),
        config::Environment::Development,
    );
    let framework = loader.load_framework()?;
    assert_eq!(framework.app.name, "openrust-server");
    assert_eq!(framework.server.port, 3000);
    assert!(framework.server.compression.enabled);
    assert!(!framework.server.cors.enabled);
    Ok(())
}

#[test]
fn unknown_route_returns_404_problem() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let response = router()
            .await?
            .oneshot(Request::get("/nope").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers()["content-type"],
            "application/problem+json"
        );
        assert!(response.headers().contains_key("x-request-id"));
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        assert!(std::str::from_utf8(&body)?.contains("\"code\":\"not_found\""));
        Ok(())
    })
}
