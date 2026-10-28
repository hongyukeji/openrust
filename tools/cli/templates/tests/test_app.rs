//! 使用与生产相同的 Provider 清单测试应用 HTTP 路由。
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../routes/web.rs"]
mod routes;

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
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let mut app = TestApp::<bootstrap::OpenRustGeneratedApplication>::builder(source)
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
            "data": {"name": env!("CARGO_PKG_NAME")}
        }))?;
    let health = Request::builder().uri("/health").body(Body::empty())?;
    app.request(health).await?.assert_status(StatusCode::OK)?;
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}
