//! 验证模块 Provider 装饰后的业务服务贯穿 boot 与真实 HTTP 注入。

use super::ExampleServiceProvider;
use crate::modules::example::services::ExampleMessage;
use openrust::foundation::ProviderBuilder;
use openrust::http::{
    HttpServiceProvider,
    axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    },
};
use tower::ServiceExt;

#[tokio::test]
async fn binding_extension_preserves_default_greeting() -> Result<(), Box<dyn std::error::Error>> {
    assert_extension(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        "Hello from modules/example",
    )
    .await
}

#[tokio::test]
async fn binding_extension_uses_module_code_config_before_first_resolve()
-> Result<(), Box<dyn std::error::Error>> {
    let deployed = tempfile::tempdir()?;
    std::fs::write(
        deployed.path().join(".env"),
        "EXAMPLE_GREETING=\"Hello from isolated module overlay\"\n",
    )?;
    assert_extension(
        deployed.path().to_path_buf(),
        "Hello from isolated module overlay",
    )
    .await
}

async fn assert_extension(
    root: std::path::PathBuf,
    expected_greeting: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(openrust::http::CookieServiceProvider::plain())
        .provider(openrust::database::DatabaseServiceProvider)
        .provider(openrust::cache::CacheServiceProvider::new())
        .provider(ExampleServiceProvider::new(root.clone()));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}}
        }),
    ));
    let manifest =
        <crate::bootstrap::Server as openrust::foundation::ApplicationDefinition>::config();
    let sources = openrust::config::ConfigLoader::application_code_sources(
        &root, "APP_", None, &manifest,
    )?
    .merge_ordered_entries(sources.extract::<serde_json::Map<String, serde_json::Value>>()?)?;
    let app = openrust::Application::from_provider_runtime(builder.start(sources).await?)?;
    let service = app.state().resolve::<ExampleMessage>().await?;
    assert_eq!(service.text(), expected_greeting);
    assert_eq!(service.binding_extension(), Some("module-provider"));

    let response = openrust::foundation::http::build_router(&app)?
        .oneshot(
            Request::get("/api/example/container-extension")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"]["message"], expected_greeting);
    assert_eq!(body["data"]["binding_extension"], "module-provider");
    assert_eq!(body["data"]["same_boot_instance"], true);

    let response = openrust::foundation::http::build_router(&app)?
        .oneshot(
            Request::get("/api/example/container-call")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"]["message"], expected_greeting);
    assert_eq!(body["data"]["label"], "container-call");
    assert_eq!(body["data"]["binding_extension"], "module-provider");

    let response = openrust::foundation::http::build_router(&app)?
        .oneshot(
            Request::get("/api/example/container-method")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(
        body["data"]["direct"],
        format!("direct: {expected_greeting}")
    );
    assert_eq!(
        body["data"]["bound"],
        format!("provider: {expected_greeting}")
    );
    assert_eq!(body["data"]["static_direct"], "direct: context");
    assert_eq!(body["data"]["static_bound"], "provider: context");

    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}
