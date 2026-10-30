//! 复制到生成应用 tests/ 后，验证生产 Provider 的 boot/HTTP 同一服务链。

#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../modules/mod.rs"]
mod modules;
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
async fn generated_provider_extends_original_factory_in_boot_and_http()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let mut app = TestApp::<GeneratedApplication>::builder(source)
        .build()
        .await?;
    app.request(Request::get("/api/generated/container-extension").body(Body::empty())?)
        .await?
        .assert_status(StatusCode::OK)?
        .assert_json(&serde_json::json!({
            "success": true,
            "code": "ok",
            "message": "ok",
            "data": {
                "message": "from original factory",
                "extended": true,
                "same_boot_instance": true
            }
        }))?;
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}
