//! 使用与生产相同的 Provider 清单测试应用 HTTP 路由。
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "support/message_bags.rs"]
mod message_bags;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "support/request_cookie.rs"]
mod request_cookie;
#[path = "support/request_input.rs"]
mod request_input;
#[path = "support/request_redirect.rs"]
mod request_redirect;
#[path = "support/request_session.rs"]
mod request_session;
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
        "[app]\nname = 'configured-application'\n[server]\nroot_target = 'view:index.html'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
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
    let page = app
        .request(Request::builder().uri("/").body(Body::empty())?)
        .await?;
    page.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(page.body())?;
    assert!(html.contains("configured-application"));
    assert!(html.contains("<p id=\"default-error\"></p>"));
    assert!(html.contains("<p id=\"other-error\"></p>"));
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

#[tokio::test]
async fn generated_application_consumes_message_bags_through_business_provider()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    message_bags::assert_message_bags::<GeneratedApplication>(source).await
}

#[tokio::test]
async fn generated_application_redirects_to_named_route_and_restores_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_redirect::assert_request_redirect::<GeneratedApplication>(source).await
}

#[tokio::test]
async fn generated_application_exposes_cookie_contract() -> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_cookie::assert_request_cookie::<GeneratedApplication>(source, false).await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_exposes_session_lifecycle() -> Result<(), Box<dyn std::error::Error>>
{
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_session::assert_request_session::<GeneratedApplication>(source, false).await?;
    Ok(())
}

#[tokio::test]
async fn generated_original_host_supports_explicit_cookie_session_without_database()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(
        "[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let source = source.merge(Serialized::defaults(
        serde_json::json!({"session":{"lottery":[1,1]}}),
    ));
    request_session::assert_original_host_cookie_session::<GeneratedApplication>(source).await
}
