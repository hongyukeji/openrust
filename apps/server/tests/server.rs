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
        let response_request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
        assert_eq!(body["success"], true);
        assert_eq!(body["code"], "ok");
        assert_eq!(body["message"], "ok");
        assert_eq!(body["data"], serde_json::json!({"status": "ok"}));
        assert_eq!(body["request_id"], response_request_id);
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
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(json["success"], true);
        assert_eq!(json["code"], "ok");
        assert_eq!(json["data"]["name"], "openrust-server");
        assert_eq!(json["data"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["request_id"], request_id);
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
    assert_eq!(framework.api.http_status, config::HttpStatusMode::AlwaysOk);
    assert!(framework.server.compression.enabled);
    assert!(!framework.server.cors.enabled);
    Ok(())
}

#[test]
fn unknown_route_returns_envelope() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let response = router()
            .await?
            .oneshot(Request::get("/nope").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert!(response.headers().contains_key("x-request-id"));
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(json["success"], false);
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["request_id"], request_id);
        Ok(())
    })
}
