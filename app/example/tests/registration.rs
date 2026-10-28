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

#[tokio::test]
async fn module_registration_controls_only_its_business_routes()
-> Result<(), Box<dyn std::error::Error>> {
    for registered in [false, true] {
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(openrust::db::DatabaseServiceProvider)
            .provider(FrameworkRoutes);
        if registered {
            builder.provider(super::providers::ExampleServiceProvider);
        }
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            openrust::config::FrameworkConfig::default(),
        ));
        let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
        let router = openrust::http::build_router(&app)?;
        for path in ["/health", "/api"] {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty())?)
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = router
            .oneshot(Request::get("/api/example").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let response: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(response["success"], registered);
        if registered {
            assert_eq!(response["data"]["message"], "Hello from app/example");
        } else {
            // 默认 AlwaysOk envelope 保持 HTTP 200，机器码仍明确没有此业务路由。
            assert_eq!(response["code"], "not_found");
        }
    }
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
            "cache": {"default_store": "default", "stores": {"default": {"driver": "memory"}}},
            "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })))
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
        bindings.instance(Arc::new(ExampleMessage::new(
            "A test-owned business message",
        )));
        Ok(())
    });
    let mut replaced = fixture.build().await?;
    for (app, expected) in [
        (&production, "Hello from app/example"),
        (&replaced, "A test-owned business message"),
    ] {
        let response = app
            .request(Request::get("/api/example").body(Body::empty())?)
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(json["data"]["message"], expected);
        assert!(json["data"]["served_at"].as_str().is_some());
    }
    production
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    replaced
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_testapp_can_remove_only_the_business_provider()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::core::ProviderId;
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut fixture = TestApp::<crate::bootstrap::Server>::builder(production_sources(true));
    fixture.remove_provider(ProviderId::of::<super::providers::ExampleServiceProvider>());
    let mut app = fixture.build().await?;
    app.request(Request::get("/api/example").body(Body::empty())?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    app.request(Request::get("/health").body(Body::empty())?)
        .await?
        .assert_status(StatusCode::OK)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}
