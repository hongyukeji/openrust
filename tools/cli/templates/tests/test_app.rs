//! 使用与生产相同的 Provider 清单测试应用 HTTP 路由。
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "support/request_input.rs"]
mod request_input;
#[path = "../routes/web.rs"]
mod routes;

use bootstrap::OpenRustGeneratedApplication as GeneratedApplication;

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::TestApp;
use std::time::Duration;

#[tokio::test]
async fn generated_application_serves_api_and_health() -> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[app]\nname = 'configured-application'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let mut app = TestApp::<GeneratedApplication>::builder(source)
        .build()
        .await?;
    let request = Request::builder().uri("/api").body(Body::empty())?;
    app.request(request)
        .await?
        .assert_status(StatusCode::OK)?
        .assert_json(&serde_json::json!({
            "success": true,
            "code": "ok",
            "message": "ok",
            "data": {"name": "configured-application"}
        }))?;
    let health = Request::builder().uri("/health").body(Body::empty())?;
    app.request(health).await?.assert_status(StatusCode::OK)?;
    let missing = Request::builder().uri("/api/missing").body(Body::empty())?;
    let fallback = app.request(missing).await?;
    fallback.assert_status(StatusCode::NOT_FOUND)?;
    assert_eq!(fallback.body(), "未找到路由：/api/missing".as_bytes());
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_exposes_request_input_to_business_provider()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_input::assert_request_input::<GeneratedApplication>(source).await
}
