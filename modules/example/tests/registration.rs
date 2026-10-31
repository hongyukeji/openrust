use openrust::http::{
    HttpServiceProvider,
    axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    },
};
use tower::ServiceExt;

struct FrameworkRoutes;
impl openrust::core::ServiceProvider for FrameworkRoutes {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(openrust::routes::framework_routes)?;
            Ok(())
        })
    }
}

#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
struct LateOidcOverlay;

#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
impl openrust::core::ServiceProvider for LateOidcOverlay {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.overlay(
                "example",
                serde_json::json!({"oidc": {
                    "issuer": "http://127.0.0.1:18761/realms/openrust",
                    "client_id": "test-client",
                    "client_secret": null,
                    "redirect_uri": "http://127.0.0.1:3000/api/example/oidc/callback",
                    "allow_loopback": true
                }}),
            )
        })
    }
}

#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[tokio::test]
async fn oidc_overlay_after_module_routes_fails_before_ready()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(openrust::db::DatabaseServiceProvider)
        .provider(super::providers::ExampleServiceProvider::new(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ))
        .provider(LateOidcOverlay);
    let error = builder
        .start(production_sources(false))
        .await
        .err()
        .ok_or("late OIDC overlay unexpectedly became ready")?;
    assert_eq!(error.code(), "example_oidc_config_changed_after_routes");
    Ok(())
}

#[tokio::test]
async fn module_registration_controls_only_its_business_routes()
-> Result<(), Box<dyn std::error::Error>> {
    for registered in [false, true] {
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(openrust::db::DatabaseServiceProvider)
            .provider(openrust::cache::CacheServiceProvider::new())
            .provider(FrameworkRoutes);
        let manifest = openrust::core::ModuleManifest::new().module(super::descriptor(
            false,
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ));
        builder.module_manifest(if registered {
            manifest.enable("example")
        } else {
            manifest
        });
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            openrust::config::FrameworkConfig::default(),
        ))
        .merge(figment::providers::Serialized::defaults(
            serde_json::json!({
                "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}}
            }),
        ));
        let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
        assert_eq!(
            app.state()
                .container()
                .contains::<super::services::ExampleMessage>(),
            registered
        );
        if registered {
            assert_eq!(
                app.section::<serde_json::Value>("example")?["greeting"],
                "Hello from modules/example"
            );
        } else {
            assert!(app.section::<serde_json::Value>("example").is_err());
        }
        let router = openrust::http::build_router(&app)?;
        for path in ["/health", "/api"] {
            let response = router
                .clone()
                .oneshot(
                    Request::get(path)
                        .header("host", "localhost")
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = router
            .oneshot(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            if registered {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let response: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(response["success"], registered);
        if registered {
            assert_eq!(response["data"]["message"], "Hello from modules/example");
        } else {
            assert_eq!(response["code"], "not_found");
        }
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
    }
    Ok(())
}

struct LateExampleMessage(&'static str);

impl openrust::core::ServiceProvider for LateExampleMessage {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(super::services::ExampleMessage::new(
                self.0,
            )))
            .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn module_business_uses_runtime_provider_rebinding_after_boot()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(openrust::db::DatabaseServiceProvider)
        .provider(openrust::cache::CacheServiceProvider::new())
        .provider(FrameworkRoutes)
        .module_manifest(
            openrust::core::ModuleManifest::new()
                .module(super::descriptor(
                    false,
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                ))
                .enable("example"),
        );
    let source = figment::Figment::from(figment::providers::Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}}
        }),
    ));
    let mut app = openrust::App::from_provider_runtime(builder.start(source).await?)?;
    let original = app
        .state()
        .resolve::<super::services::ExampleMessage>()
        .await?;
    assert_eq!(original.text(), "Hello from modules/example");
    let first = app
        .register_provider(LateExampleMessage("已登记业务消息"))
        .await?;
    assert_eq!(
        first,
        app.register_provider(LateExampleMessage("重复消息"))
            .await?
    );
    assert_eq!(
        app.state()
            .resolve::<super::services::ExampleMessage>()
            .await?
            .text(),
        "已登记业务消息"
    );
    assert_eq!(
        first,
        app.force_register_provider(LateExampleMessage("强制更新消息"))
            .await?
    );
    assert_eq!(original.text(), "Hello from modules/example");
    assert_eq!(
        app.state()
            .resolve::<super::services::ExampleMessage>()
            .await?
            .text(),
        "强制更新消息"
    );
    let response = openrust::http::build_router(&app)?
        .oneshot(
            Request::get("/api/example")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"]["message"], "强制更新消息");
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

fn production_sources(semantic: bool) -> figment::Figment {
    let mut framework = openrust::config::FrameworkConfig::default();
    if semantic {
        framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    }
    figment::Figment::from(figment::providers::Serialized::defaults(framework))
        .merge(figment::providers::Serialized::defaults(serde_json::json!({
            "app": {"env": "test"},
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
            "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })))
}

async fn assert_initial_config_greeting(
    app: &mut openrust::testing::TestApp<crate::bootstrap::Server>,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .request(
            Request::get("/api/example/config-greeting?key=security.encryption.keys")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(
        body["data"],
        serde_json::json!({"greeting": "Hello from modules/example"})
    );
    assert!(!String::from_utf8_lossy(response.body()).contains("KioqKioq"));
    Ok(())
}

async fn assert_config_feature(
    app: &mut openrust::testing::TestApp<crate::bootstrap::Server>,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .request(
            Request::get("/api/example/config-feature")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["data"], serde_json::json!({"enabled": true}));
    Ok(())
}

#[tokio::test]
async fn production_config_greeting_reads_only_the_module_business_key()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let repository = openrust::config::repository::ConfigRepository::new(figment::Figment::from(
        figment::providers::Serialized::defaults(serde_json::json!({
            "example": {"greeting": "仓储中的业务问候"}
        })),
    ));
    assert_eq!(
        super::services::config_reading::greeting(&repository)?,
        "仓储中的业务问候"
    );
    let missing = openrust::config::repository::ConfigRepository::new(figment::Figment::from(
        figment::providers::Serialized::defaults(serde_json::json!({"other": true})),
    ));
    assert_eq!(
        super::services::config_reading::greeting(&missing)?,
        "示例问候语尚未配置"
    );
    assert!(!super::services::config_reading::feature_enabled(&missing)?);
    missing.set("example.greeting", serde_json::json!(7))?;
    assert!(super::services::config_reading::greeting(&missing).is_err());

    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    assert_config_feature(&mut app).await?;
    let mut other = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    let startup_greeting = app
        .app()?
        .state()
        .get::<super::services::ExampleMessage>()
        .ok_or("启动时未解析示例业务服务")?
        .text()
        .to_owned();
    assert_initial_config_greeting(&mut app).await?;
    let reviewed = app
        .request(
            Request::post("/api/example/config-greeting/review?key=security.encryption.keys")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    reviewed.assert_status(StatusCode::OK)?;
    let reviewed_body: serde_json::Value = serde_json::from_slice(reviewed.body())?;
    assert_eq!(
        reviewed_body["data"],
        serde_json::json!({"greeting": "示例问候语已复核"})
    );
    assert_eq!(
        app.app()?.config_repository().get("example.greeting")?,
        Some(serde_json::json!("示例问候语已复核"))
    );
    assert_eq!(
        app.app()?
            .section::<super::services::ExampleConfig>("example")?
            .greeting,
        "示例问候语已复核"
    );
    let refreshed = app
        .request(
            Request::get("/api/example/config-greeting")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    refreshed.assert_status(StatusCode::OK)?;
    let refreshed_body: serde_json::Value = serde_json::from_slice(refreshed.body())?;
    assert_eq!(refreshed_body["data"], reviewed_body["data"]);
    assert_eq!(
        app.app()?
            .state()
            .get::<super::services::ExampleMessage>()
            .ok_or("复核后业务服务意外丢失")?
            .text(),
        startup_greeting
    );
    let independent = other
        .request(
            Request::get("/api/example/config-greeting")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    independent.assert_status(StatusCode::OK)?;
    let independent_body: serde_json::Value = serde_json::from_slice(independent.body())?;
    assert_eq!(
        independent_body["data"],
        serde_json::json!({"greeting": "Hello from modules/example"})
    );
    other
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn ordered_business_source_survives_module_overlay_and_runtime_set()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::config::ConfigSources;
    use openrust::testing::TestApp;
    use std::time::Duration;

    let defaults: serde_json::Value = production_sources(false).extract()?;
    let defaults = defaults.as_object().ok_or("默认配置根必须是对象")?;
    let mut entries = defaults
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    entries.push((
        "example".to_owned(),
        serde_json::json!({"greeting": "来源中的问候", "article_summary_origin": null}),
    ));
    let sources = ConfigSources::from_ordered_entries(entries)?;
    let mut app = TestApp::<crate::bootstrap::Server>::builder(sources.clone())
        .build()
        .await?;
    let mut other = TestApp::<crate::bootstrap::Server>::builder(sources)
        .build()
        .await?;
    let keys = |app: &TestApp<crate::bootstrap::Server>| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let all = app.app()?.config_repository().all()?;
        let section = all["example"].as_object().ok_or("example 区段必须是对象")?;
        Ok(section.keys().cloned().collect())
    };
    let first_two = |app: &TestApp<crate::bootstrap::Server>| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        Ok(keys(app)?.into_iter().take(2).collect())
    };
    assert_eq!(first_two(&app)?, ["greeting", "article_summary_origin"]);
    assert_eq!(
        app.app()?.config_repository().get("example.greeting")?,
        Some(serde_json::json!("Hello from modules/example"))
    );
    let feature = app
        .request(
            Request::get("/api/example/config-feature")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    feature.assert_status(StatusCode::OK)?;
    let feature_body: serde_json::Value = serde_json::from_slice(feature.body())?;
    assert_eq!(feature_body["data"], serde_json::json!({"enabled": true}));
    app.app()?
        .config_repository()
        .set("example.greeting", serde_json::json!("当前应用已复核"))?;
    assert_eq!(first_two(&app)?, ["greeting", "article_summary_origin"]);
    assert_eq!(
        app.app()?
            .section::<super::services::ExampleConfig>("example")?
            .greeting,
        "当前应用已复核"
    );
    let response = app
        .request(
            Request::get("/api/example/config-greeting")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(
        body["data"],
        serde_json::json!({"greeting": "当前应用已复核"})
    );
    assert_eq!(
        other.app()?.config_repository().get("example.greeting")?,
        Some(serde_json::json!("Hello from modules/example"))
    );
    assert_eq!(first_two(&other)?, ["greeting", "article_summary_origin"]);
    other
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_request_representation_negotiates_html_json_and_rejection()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let sources = production_sources(false).merge(figment::providers::Serialized::defaults(
        serde_json::json!({"api": {"http_status": "always_ok"}}),
    ));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(sources)
        .build()
        .await?;
    for (accept, status, content_type) in [
        (None, StatusCode::OK, "text/html"),
        (Some(""), StatusCode::OK, "text/html"),
        (Some("text/html"), StatusCode::OK, "text/html"),
        (Some("application/json"), StatusCode::OK, "application/json"),
        (
            Some("text/html;q=0.2, application/json;q=0.9"),
            StatusCode::OK,
            "application/json",
        ),
        (
            Some("text/html;q=0.9, application/json;q=0.2"),
            StatusCode::OK,
            "text/html",
        ),
        (Some("text/plain"), StatusCode::OK, "application/json"),
    ] {
        let mut request = Request::get("/api/example/representation").header("host", "localhost");
        if let Some(accept) = accept {
            request = request.header("accept", accept);
        }
        let response = app.request(request.body(Body::empty())?).await?;
        assert_eq!(
            response.status(),
            status,
            "Accept {accept:?}; actual HTTP status: {}",
            response.status()
        );
        assert_eq!(response.headers()["vary"], "Accept");
        assert!(
            response.headers()["content-type"]
                .to_str()?
                .starts_with(content_type),
            "Accept {accept:?}"
        );
        if content_type == "text/html" {
            let html = std::str::from_utf8(response.body())?;
            assert!(html.contains("Hello from modules/example"));
        } else {
            let json: serde_json::Value = serde_json::from_slice(response.body())?;
            if accept == Some("text/plain") {
                assert_eq!(json["code"], "not_acceptable");
            } else {
                assert_eq!(json["data"]["message"], "Hello from modules/example");
            }
        }
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_request_representation_uses_semantic_406_when_configured()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(true))
        .build()
        .await?;
    let response = app
        .request(
            Request::get("/api/example/representation")
                .header("host", "localhost")
                .header("accept", "text/plain")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::NOT_ACCEPTABLE)?;
    assert_eq!(response.headers()["vary"], "Accept");
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(json["code"], "not_acceptable");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_request_info_leaves_json_body_for_typed_extractor()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    let response = app
        .request(
            Request::post("/api/example/request-body")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"完整 JSON 正文 <>&"}"#))?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(json["data"]["path"], "api/example/request-body");
    assert_eq!(json["data"]["method"], "POST");
    assert_eq!(json["data"]["is_json"], true);
    assert_eq!(json["data"]["title"], "完整 JSON 正文 <>&");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_html_representation_escapes_business_message()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::{sync::Arc, time::Duration};

    let mut builder = TestApp::<crate::bootstrap::Server>::builder(production_sources(false));
    builder.configure_after_register(|bindings| {
        Box::pin(async move {
            bindings
                .instance(Arc::new(super::services::ExampleMessage::new(
                    "<script>alert('x' & \"y\")</script>",
                )))
                .await?;
            Ok(())
        })
    });
    let mut app = builder.build().await?;
    let response = app
        .request(
            Request::get("/api/example/representation")
                .header("host", "localhost")
                .header("accept", "text/html")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(response.body())?;
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39; &amp; &quot;y&quot;)&lt;/script&gt;"));
    assert!(!html.contains("<script>"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_testapp_replaces_business_dependency_without_changing_routes()
-> Result<(), Box<dyn std::error::Error>> {
    use super::services::ExampleMessage;
    use openrust::testing::TestApp;
    use std::sync::Arc;
    use std::time::Duration;

    let mut production = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    let mut fixture = TestApp::<crate::bootstrap::Server>::builder(production_sources(false));
    fixture.configure_after_register(|bindings| {
        Box::pin(async move {
            bindings
                .instance(Arc::new(ExampleMessage::new(
                    "A test-owned business message",
                )))
                .await?;
            Ok(())
        })
    });
    let mut replaced = fixture.build().await?;
    for (app, expected) in [
        (&production, "Hello from modules/example"),
        (&replaced, "A test-owned business message"),
    ] {
        let response = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(json["data"]["message"], expected);
        assert!(json["data"]["served_at"].as_str().is_some());
    }
    let translated = production
        .request(
            Request::get("/api/example/heartbeat-label")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    translated.assert_status(StatusCode::OK)?;
    let translated: serde_json::Value = translated.json()?;
    assert_eq!(translated["data"]["message"], "Heartbeat completed");
    assert_eq!(translated["data"]["locale"], "en");
    assert!(translated["data"]["number"].as_str().is_some());
    let chinese = production
        .request(
            Request::get("/api/example/heartbeat-label")
                .header("Accept-Language", "zh-CN")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    let chinese: serde_json::Value = chinese.json()?;
    assert_eq!(chinese["data"]["locale"], "zh-CN");
    assert_eq!(chinese["data"]["message"], "心跳任务已完成");
    production
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    replaced
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_module_domain_route_binds_host_path_and_named_url()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    let response = app
        .request(
            Request::get("/api/example/domain/orders/a%20b")
                .header("host", "Acme.example.test")
                .header("x-forwarded-host", "evil.test")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = response.json()?;
    assert_eq!(json["data"]["account"], "acme");
    assert_eq!(json["data"]["id"], "a b");
    assert_eq!(
        json["data"]["url"],
        "http://acme.example.test:3000/api/example/domain/orders/a%20b"
    );
    let wrong_host = app
        .request(
            Request::get("/api/example/domain/orders/a%20b")
                .header("host", "unrelated.test")
                .body(Body::empty())?,
        )
        .await?;
    wrong_host.assert_status(StatusCode::NOT_FOUND)?;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_testapp_can_disable_only_the_business_module()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut fixture = TestApp::<crate::bootstrap::Server>::builder(production_sources(true));
    fixture.remove_module("example");
    let mut app = fixture.build().await?;
    app.request(
        Request::get("/api/example")
            .header("host", "localhost")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::NOT_FOUND)?;
    app.request(
        Request::get("/health")
            .header("host", "localhost")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct OrdersConsumerProvider;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl openrust::core::ServiceProvider for OrdersConsumerProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.resolve::<super::services::ExampleMessage>().await?;
            app.routes(|routes| routes.get("/api/orders", orders_index))?;
            Ok(())
        })
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn orders_index(
    openrust::http::Inject(message): openrust::http::Inject<super::services::ExampleMessage>,
) -> openrust::http::Json<serde_json::Value> {
    openrust::http::Json(serde_json::json!({"producer": message.text()}))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
async fn dependent_business_module_serves_http_with_real_database()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::core::{ModuleDescriptor, ModuleManifest, ProviderBuilder};
    use openrust::db::sea_orm::{ConnectionTrait, Statement};
    use openrust::testing::TestDatabase;
    use std::path::PathBuf;
    use std::time::Duration;

    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping dependent module HTTP test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let consumer_root = tempfile::tempdir()?;
    let application_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sources = production_sources(false).merge(figment::providers::Serialized::defaults(
        serde_json::json!({"database": {"url": database.url()}}),
    ));

    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(openrust::db::DatabaseServiceProvider)
        .provider(openrust::cache::CacheServiceProvider::new())
        .module_manifest(
            ModuleManifest::new()
                .module(super::descriptor(false, application_root))
                .module(
                    ModuleDescriptor::new("orders", consumer_root.path(), -100)
                        .requires("example")
                        .provider(OrdersConsumerProvider),
                )
                .enable("orders")
                .enable("example"),
        );
    let runtime = builder.start(sources).await?;
    let app = openrust::core::App::from_provider_runtime(runtime)?;
    let database_handle = app.state().resolve::<openrust::Db>().await?;
    let connection = database_handle.primary_connection()?;
    connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT 1".to_owned(),
        ))
        .await?
        .ok_or("real database did not return SELECT 1")?;

    let router = openrust::http::build_router(&app)?;
    for (path, expected) in [
        ("/api/example", "Hello from modules/example"),
        ("/api/orders", "Hello from modules/example"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        if path == "/api/example" {
            assert_eq!(json["data"]["message"], expected);
        } else {
            assert_eq!(json["data"]["producer"], expected);
        }
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}

#[cfg(feature = "view")]
#[tokio::test]
async fn production_testapp_renders_module_namespaced_view()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let application_views = tempfile::tempdir()?;
    let source = production_sources(false).merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "view": {"directory": application_views.path(), "auto_reload": false}
        }),
    ));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(source)
        .build()
        .await?;
    let response = app
        .request(
            Request::get("/example/pages/about")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(response.body())?;
    assert!(html.contains("关于示例应用"));
    assert!(html.contains("标题：</p>"));
    assert!(html.contains("摘要：</p>"));
    assert!(html.contains("不存在的字段：</p>"));
    assert!(!html.contains("<ul>"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct EmptyMigrator;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl openrust::database::sea_orm_migration::MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn module_migration_sources(database: &openrust::testing::TestDatabase) -> figment::Figment {
    production_sources(false).merge(figment::providers::Serialized::defaults(
            serde_json::json!({
                "database": {"url": database.url()},
                "auth": {"jwt": {
                    "issuer": "http://openrust.test", "audience": "module-migration-test",
                    "active_key_id": "test", "keys": {"test": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI="}
                }},
                "security": {"signed_links": {
                    "issuer": "http://openrust.test", "active_key_id": "test",
                    "keys": {"test": "Q0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0M="}
                }}
            }),
        ))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn migration_versions(
    connection: &openrust::db::DatabaseConnection,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use openrust::db::sea_orm::{ConnectionTrait, Statement};
    connection
        .query_all_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT version FROM openrust_migrations ORDER BY version".to_owned(),
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "version").map_err(Into::into))
        .collect()
}
#[cfg(all(
    feature = "session",
    any(feature = "db-postgres", feature = "db-mysql")
))]
async fn session_gc_indexes(
    connection: &openrust::db::DatabaseConnection,
    table: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use openrust::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
    let sql = match connection.get_database_backend() {
        DbBackend::Postgres => {
            "SELECT i.relname AS name FROM pg_index x JOIN pg_class i ON i.oid=x.indexrelid JOIN pg_class t ON t.oid=x.indrelid JOIN pg_namespace n ON n.oid=t.relnamespace JOIN pg_am m ON m.oid=i.relam JOIN pg_attribute a ON a.attrelid=t.oid AND a.attnum=x.indkey[0] WHERE n.nspname=current_schema() AND t.relname=$1 AND left(i.relname,length('ix_sessions_expiry_'))='ix_sessions_expiry_' AND m.amname='btree' AND NOT x.indisunique AND x.indisvalid AND x.indisready AND x.indnatts=1 AND x.indnkeyatts=1 AND x.indpred IS NULL AND x.indexprs IS NULL AND a.attname='expires_at' ORDER BY i.relname"
        }
        DbBackend::MySql => {
            "SELECT index_name AS name FROM information_schema.statistics WHERE table_schema=DATABASE() AND table_name=? AND LEFT(index_name,CHAR_LENGTH('ix_sessions_expiry_'))='ix_sessions_expiry_' GROUP BY index_name HAVING COUNT(*)=1 AND MIN(seq_in_index)=1 AND MAX(seq_in_index)=1 AND MIN(non_unique)=1 AND MAX(non_unique)=1 AND MIN(index_type)='BTREE' AND MAX(index_type)='BTREE' AND MAX(column_name)='expires_at' AND MAX(sub_part) IS NULL ORDER BY index_name"
        }
        _ => return Err("不支持的迁移测试数据库".into()),
    };
    connection
        .query_all_raw(Statement::from_sql_and_values(
            connection.get_database_backend(),
            sql,
            [table.into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "name").map_err(Into::into))
        .collect()
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
#[allow(clippy::too_many_lines)] // 同一隔离库验证启用、迁移与禁用后历史留存。
async fn module_migration_runs_with_framework_and_disable_keeps_existing_table()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::database::sea_orm::{ConnectionTrait, Statement};
    use openrust::testing::{TestApp, TestDatabase};
    use std::time::Duration;

    fn migration_command(
        phase: &'static str,
        result: openrust::Result<()>,
    ) -> openrust::Result<()> {
        result.inspect_err(|error| {
            eprintln!(
                "module_migration phase={phase} code={} status={}",
                error.code(),
                error.status()
            );
        })
    }

    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping module migration test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    eprintln!("module_migration phase=setup");
    let database = TestDatabase::create::<EmptyMigrator>(&admin_url)
        .await
        .inspect_err(|error| {
            eprintln!(
                "module_migration phase=create_database code={} status={}",
                error.code(),
                error.status()
            );
        })?;
    eprintln!("module_migration phase=database_created");
    let sources = || module_migration_sources(&database);
    migration_command(
        "initial_migrate",
        TestApp::<crate::bootstrap::Server>::builder(sources())
            .run_command("migrate", Vec::new())
            .await,
    )?;
    let mut enabled = TestApp::<crate::bootstrap::Server>::builder(sources())
        .build()
        .await
        .inspect_err(|error| {
            eprintln!(
                "module_migration phase=build_app code={} status={}",
                error.code(),
                error.status()
            );
        })?;
    eprintln!("module_migration phase=read_initial");
    let connection = database.connection()?;
    let table = |name| {
        Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT id FROM {name} LIMIT 1"),
        )
    };
    let statement = table("example_heartbeats");
    let articles = table("example_articles");
    let article_events = table("example_article_events");
    let showcase_categories = table("example_record_categories");
    let showcase_records = table("example_records");
    let attachment_cleanup = Statement::from_string(
        connection.get_database_backend(),
        "SELECT article_id FROM example_attachment_cleanup LIMIT 1".to_owned(),
    );
    let article_imports = Statement::from_string(
        connection.get_database_backend(),
        "SELECT import_id FROM example_article_imports LIMIT 1".to_owned(),
    );
    let private_notes = Statement::from_string(
        connection.get_database_backend(),
        "SELECT private_note_ciphertext FROM example_articles LIMIT 1".to_owned(),
    );
    connection.query_one_raw(statement.clone()).await?;
    connection.query_one_raw(articles.clone()).await?;
    connection.query_one_raw(article_events.clone()).await?;
    connection.query_one_raw(private_notes.clone()).await?;
    connection.query_one_raw(attachment_cleanup.clone()).await?;
    connection.query_one_raw(article_imports.clone()).await?;
    connection
        .query_one_raw(showcase_categories.clone())
        .await?;
    connection.query_one_raw(showcase_records.clone()).await?;
    let shared_history = Statement::from_string(
        connection.get_database_backend(),
        "SELECT version FROM openrust_migrations".to_owned(),
    );
    let before_history = migration_versions(&connection).await?;
    let before_down = before_history.len();
    #[cfg(feature = "session")]
    let gc_indexes = {
        use openrust::database::sea_orm_migration::MigrationName;
        assert_eq!(
            before_history.last().map(String::as_str),
            Some(openrust::session::SessionGcMigration.name())
        );
        let db = enabled.app()?.state().resolve::<openrust::db::Db>().await?;
        let indexes = session_gc_indexes(&connection, &db.table_name("sessions")?).await?;
        assert_eq!(indexes.len(), 1);
        migration_command(
            "session_gc_down",
            enabled
                .app()?
                .run_command("migrate", vec!["down".to_owned(), "1".to_owned()])
                .await,
        )?;
        eprintln!("module_migration phase=read_after_session_gc_down");
        assert_eq!(
            migration_versions(&connection).await?,
            before_history[..before_down - 1]
        );
        assert_eq!(
            session_gc_indexes(&connection, &db.table_name("sessions")?).await?,
            Vec::<String>::new()
        );
        connection
            .query_one_raw(showcase_categories.clone())
            .await?;
        connection.query_one_raw(showcase_records.clone()).await?;
        connection.query_one_raw(statement.clone()).await?;
        connection.query_one_raw(articles.clone()).await?;
        connection.query_one_raw(article_events.clone()).await?;
        connection.query_one_raw(private_notes.clone()).await?;
        connection.query_one_raw(attachment_cleanup.clone()).await?;
        connection.query_one_raw(article_imports.clone()).await?;
        indexes
    };
    let business_history = migration_versions(&connection).await?;
    migration_command(
        "business_down",
        enabled
            .app()?
            .run_command("migrate", vec!["down".to_owned(), "1".to_owned()])
            .await,
    )?;
    eprintln!("module_migration phase=read_after_business_down");
    connection.query_one_raw(statement.clone()).await?;
    connection.query_one_raw(articles.clone()).await?;
    connection.query_one_raw(article_events.clone()).await?;
    connection.query_one_raw(private_notes.clone()).await?;
    connection.query_one_raw(attachment_cleanup.clone()).await?;
    connection.query_one_raw(article_imports.clone()).await?;
    assert!(
        connection
            .query_one_raw(showcase_records.clone())
            .await
            .is_err()
    );
    assert!(
        connection
            .query_one_raw(showcase_categories.clone())
            .await
            .is_err()
    );
    assert_eq!(
        connection
            .query_all_raw(shared_history.clone())
            .await?
            .len()
            + 1,
        business_history.len()
    );
    assert_eq!(
        migration_versions(&connection).await?,
        business_history[..business_history.len() - 1]
    );
    migration_command(
        "business_up",
        enabled
            .app()?
            .run_command("migrate", vec!["up".to_owned(), "1".to_owned()])
            .await,
    )?;
    eprintln!("module_migration phase=read_after_business_up");
    connection.query_one_raw(statement.clone()).await?;
    connection.query_one_raw(articles.clone()).await?;
    connection.query_one_raw(article_events.clone()).await?;
    connection.query_one_raw(private_notes.clone()).await?;
    connection.query_one_raw(attachment_cleanup.clone()).await?;
    connection.query_one_raw(article_imports.clone()).await?;
    connection
        .query_one_raw(showcase_categories.clone())
        .await?;
    connection.query_one_raw(showcase_records.clone()).await?;
    assert_eq!(migration_versions(&connection).await?, business_history);
    #[cfg(feature = "session")]
    {
        let db = enabled.app()?.state().resolve::<openrust::db::Db>().await?;
        assert_eq!(
            session_gc_indexes(&connection, &db.table_name("sessions")?).await?,
            Vec::<String>::new()
        );
        migration_command(
            "session_gc_up",
            enabled
                .app()?
                .run_command("migrate", vec!["up".to_owned(), "1".to_owned()])
                .await,
        )?;
        eprintln!("module_migration phase=read_after_session_gc_up");
        assert_eq!(
            session_gc_indexes(&connection, &db.table_name("sessions")?).await?,
            gc_indexes
        );
        connection
            .query_one_raw(showcase_categories.clone())
            .await?;
        connection.query_one_raw(showcase_records.clone()).await?;
    }
    assert_eq!(migration_versions(&connection).await?, before_history);
    assert_eq!(
        connection.query_all_raw(shared_history).await?.len(),
        before_down
    );
    eprintln!("module_migration phase=shutdown_enabled");
    enabled
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;

    let mut disabled = TestApp::<crate::bootstrap::Server>::builder(sources());
    disabled.remove_module("example");
    let mut disabled = disabled.build().await?;
    migration_command(
        "disabled_migrate",
        disabled.app()?.run_command("migrate", Vec::new()).await,
    )?;
    eprintln!("module_migration phase=read_disabled");
    connection.query_one_raw(statement).await?;
    connection.query_one_raw(articles).await?;
    connection.query_one_raw(article_events).await?;
    connection.query_one_raw(private_notes).await?;
    connection.query_one_raw(attachment_cleanup).await?;
    connection.query_one_raw(article_imports).await?;
    connection.query_one_raw(showcase_categories).await?;
    connection.query_one_raw(showcase_records).await?;
    disabled
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn production_input_normalizes_json_and_preserves_source_and_raw_content()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    let body = serde_json::json!({
        "title": "  body title  ", "profile": {"name": "  Alice  "}, "nullable": "  ",
        "password": "  private-sentinel  ", "user": {"password": "  nested-sentinel  "},
        "password_confirmation": "",
        "typed": {
            "title":"  demo  ", "quantity":"12", "amount":"1.25x", "subscribed":"YES",
            "publication":"published", "publications":{"2":"draft","5":"unknown","9":"published"},
            "items":{"2":"a","5":"b"}, "profile":{"display_name":" Alice "}, "note":"0",
            "published_at":"2020-01-01T16:30:25+00:00", "calendar_date":"2020-01-02",
            "duration":"90 minutes", "delay":75
        }
    })
    .to_string();
    let response = app
        .request(
            Request::post(
                "/api/example/input?title=query&nullable=from-query&fallback=%20query-only%20&page=7&label=%20query%20",
            )
            .header("host", "localhost")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    let data = &json["data"];
    assert_eq!(
        data["selected"],
        serde_json::json!({"title": "body title", "profile": {"name": "Alice"}})
    );
    assert_eq!(
        data["typed"],
        serde_json::json!({
            "title":"demo", "quantity":12, "amount":1.25, "subscribed":true,
            "bounded_quantity":10, "publication":"published", "publications":{"2":"draft","9":"published"},
            "items":{"2":"a","5":"b"}, "selected":{"typed":{"title":"demo","quantity":"12"}},
            "profile_display_name":"Alice", "note_produced_value":false,
            "published_at":"2020-01-01T16:30:25+00:00", "calendar_date":"2020-01-02T00:00:00+00:00",
            "duration":{"years":0,"months":0,"days":0,"hours":0,"minutes":90,"seconds":0,"microseconds":0,"invert":false,"total_days":null},
            "delay_minutes":75
        })
    );
    assert_eq!(data["title"], "body title");
    assert_eq!(data["profile_name"], "Alice");
    assert_eq!(data["query_title"], "query");
    assert_eq!(data["json_title"], "body title");
    assert!(data["form_title"].is_null());
    assert_eq!(data["query_page"], 7);
    assert_eq!(data["query_label"], "query");
    assert_eq!(
        data["checks"],
        serde_json::json!({
            "has_nullable": true, "nullable_filled": false, "has_any": true, "missing": true,
            "is_not_filled": true, "any_filled": true, "password_has_outer_spaces": true,
            "nested_password_has_outer_spaces": false, "empty_password_is_null": true
        })
    );
    assert_eq!(data["excluded_title"], true);
    assert_eq!(data["source_has_title"], true);
    assert_eq!(
        data["merged"],
        serde_json::json!({"title": "  revised  ", "profile": {"role": "reader"}, "generated": "fallback"})
    );
    assert_eq!(data["replaced_title"], "replacement");
    assert_eq!(data["query_fallback"], "query-only");
    assert_eq!(data["content_bytes"], body.len());
    assert_eq!(data["content_preserved"], true);
    assert!(!std::str::from_utf8(response.body())?.contains("sentinel"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_input_parses_php_form_arrays_and_mixed_keys()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    let response = app.request(
        Request::post("/api/example/input?title=query&fallback=still-query")
            .header("host", "localhost")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("title=%20form%20&profile[name]=%20Bob%20&items[]=%20first%20&items[]=second&php_keys[-1]=negative&php_keys[00]=leading&php_keys[2]=sparse&nullable="))?
    ).await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(
        json["data"]["selected"],
        serde_json::json!({
            "title": "form", "profile": {"name": "Bob"}, "items": ["first", "second"],
            "php_keys": {"-1": "negative", "00": "leading", "2": "sparse"}
        })
    );
    assert_eq!(json["data"]["form_title"], "form");
    assert_eq!(json["data"]["query_title"], "query");
    assert_eq!(json["data"]["checks"]["has_nullable"], true);
    assert_eq!(json["data"]["checks"]["nullable_filled"], false);
    assert_eq!(json["data"]["query_fallback"], "still-query");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_input_raw_policy_is_scoped_and_typed_json_keeps_default_normalization()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    for (path, expected) in [
        ("/api/example/input-raw", "  title  "),
        ("/api/example/request-body", "title"),
        ("/api/example/input-raw", "  title  "),
    ] {
        let response = app
            .request(
                Request::post(path)
                    .header("host", "localhost")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"  title  ","nullable":""}"#))?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = serde_json::from_slice(response.body())?;
        assert_eq!(json["data"]["title"], expected);
        if path.ends_with("input-raw") {
            assert_eq!(json["data"]["nullable"], "");
        }
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

// 捕获原始 URI 被 GET 输入修改覆盖、POST 正文泄入查询以及敏感查询被回显的回归。
#[tokio::test]
async fn production_request_url_uses_original_uri_and_current_query_bag()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;
    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    let base = "http://localhost:8080/api/example/request-url";
    let original = format!("{base}?label=public&page=1");
    for (method, json_source) in [("GET", false), ("GET", true), ("POST", true)] {
        let mut builder = Request::builder()
            .method(method)
            .uri("/api/example/request-url?page=1&label=public")
            .header("host", "localhost:8080")
            .header("purpose", "prefetch")
            .header("authorization", "Bearer never-echo")
            .header("cookie", "private=never-echo");
        if json_source {
            builder = builder.header("content-type", "application/json");
        }
        let response = app
            .request(builder.body(if json_source {
                Body::from(r#"{"secret":"never-echo"}"#)
            } else {
                Body::empty()
            })?)
            .await?;
        let mutates_query = method == "GET" && !json_source;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = serde_json::from_slice(response.body())?;
        let data = &json["data"];
        assert_eq!(data["address"]["root"], "http://localhost:8080");
        assert_eq!(data["address"]["url"], base);
        assert_eq!(data["address"]["full_url"], original);
        assert_eq!(data["address"]["scheme"], "http");
        assert_eq!(data["address"]["http_host"], "localhost:8080");
        assert_eq!(data["address"]["port"], 8080);
        assert_eq!(data["address"]["decoded_path"], "api/example/request-url");
        assert_eq!(
            data["address"]["segments"],
            serde_json::json!(["api", "example", "request-url"])
        );
        assert_eq!(data["address"]["first_segment"], "api");
        assert_eq!(data["address"]["zero_segment"], serde_json::Value::Null);
        assert_eq!(data["address"]["missing_segment"], serde_json::Value::Null);
        assert_eq!(data["address"]["is_example"], true);
        assert_eq!(data["address"]["full_url_is_example"], true);
        assert_eq!(data["address"]["prefetch"], true);
        assert_eq!(
            data["address"]["with_query"],
            format!("{base}?page=2&label=public")
        );
        assert_eq!(
            data["address"]["without_query"],
            format!("{base}?label=public")
        );
        assert_eq!(data["merged"]["full_url"], original);
        assert_eq!(data["replaced_full_url"], original);
        let page = if mutates_query { 3 } else { 1 };
        assert_eq!(
            data["merged"]["with_query"],
            format!("{base}?page={page}&label=public&view=compact")
        );
        assert_eq!(
            data["merged"]["without_query"],
            format!("{base}?page={page}")
        );
        let replacement = if mutates_query {
            format!("{base}?page=4&view=compact")
        } else {
            format!("{base}?page=1&label=public&view=compact")
        };
        assert_eq!(data["replaced_with_query"], replacement);
        assert!(!String::from_utf8_lossy(response.body()).contains("never-echo"));
    }
    let response = app
        .request(
            Request::get("/api/example/request-url?token=never-echo")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::BAD_REQUEST)?;
    assert!(!String::from_utf8_lossy(response.body()).contains("never-echo"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

// 捕获模块装配丢失路由名、参数被输入覆盖，以及业务错误或 middleware 短路抹去匹配上下文。
#[tokio::test]
async fn production_route_context_keeps_matched_identity_across_business_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;
    let mut app = TestApp::<crate::bootstrap::Server>::builder(production_sources(false))
        .build()
        .await?;
    for (kind, named) in [("named", true), ("anonymous", false)] {
        let response = app
            .request(
                Request::get(format!(
                    "/api/example/route-context/{kind}/42?id=query&absent=query"
                ))
                .header("host", "localhost")
                .header("x-route-name", "forged")
                .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = serde_json::from_slice(response.body())?;
        let data = &json["data"];
        assert_eq!(
            data["name"],
            if named {
                serde_json::json!("example.route-context.show")
            } else {
                serde_json::Value::Null
            }
        );
        assert_eq!(data["is_example"], named);
        assert_eq!(
            data["uri"],
            format!("api/example/route-context/{kind}/{{id}}")
        );
        assert_eq!(data["id"], "42");
        assert_eq!(data["absent"], serde_json::Value::Null);
        assert_eq!(data["default"], "public-default");
        assert_eq!(data["parameters"], serde_json::json!([["id", "42"]]));
        assert_eq!(
            data["methods"],
            if named {
                serde_json::json!(["GET", "HEAD", "POST"])
            } else {
                serde_json::json!(["GET", "HEAD"])
            }
        );
    }
    for (id, status) in [
        ("missing", StatusCode::NOT_FOUND),
        ("blocked", StatusCode::FORBIDDEN),
    ] {
        let response = app
            .request(
                Request::get(format!("/api/example/route-context/named/{id}"))
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(status)?;
        for header in ["x-example-route-before", "x-example-route-after"] {
            assert_eq!(response.headers()[header], "example.route-context.show");
        }
    }
    let response = app
        .request(
            Request::post("/api/example/route-context/named/43?id=query")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"id":"body","absent":"body"}"#))?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(json["data"]["id"], "43");
    assert_eq!(json["data"]["default"], "public-default");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}
