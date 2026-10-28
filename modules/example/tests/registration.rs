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
            assert_eq!(response["data"]["message"], "Hello from modules/example");
        } else {
            // 默认 AlwaysOk envelope 保持 HTTP 200，机器码仍明确没有此业务路由。
            assert_eq!(response["code"], "not_found");
        }
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
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
        (&production, "Hello from modules/example"),
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
    let translated = production
        .request(Request::get("/api/example/heartbeat-label").body(Body::empty())?)
        .await?;
    translated.assert_status(StatusCode::OK)?;
    let translated: serde_json::Value = translated.json()?;
    assert_eq!(translated["data"]["message"], "Heartbeat completed");
    production
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    replaced
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
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
        .request(Request::get("/pages/about").body(Body::empty())?)
        .await?;
    response.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(response.body())?;
    assert!(html.contains("关于示例应用"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
async fn module_migration_runs_with_framework_and_disable_keeps_existing_table()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::database::sea_orm::{ConnectionTrait, Statement};
    use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
    use openrust::testing::{TestApp, TestDatabase};
    use std::time::Duration;

    struct EmptyMigrator;
    #[openrust::database::sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }

    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping module migration test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let sources = || {
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
    };
    let mut enabled = TestApp::<crate::bootstrap::Server>::builder(sources())
        .build()
        .await?;
    enabled.app()?.run_command("migrate", Vec::new()).await?;
    let connection = database.connection()?;
    let statement = Statement::from_string(
        connection.get_database_backend(),
        "SELECT id FROM example_heartbeats LIMIT 1".to_owned(),
    );
    connection.query_one_raw(statement.clone()).await?;
    let shared_history = Statement::from_string(
        connection.get_database_backend(),
        "SELECT version FROM openrust_migrations".to_owned(),
    );
    let before_down = connection
        .query_all_raw(shared_history.clone())
        .await?
        .len();
    enabled
        .app()?
        .run_command("migrate", vec!["down".to_owned(), "1".to_owned()])
        .await?;
    assert!(connection.query_one_raw(statement.clone()).await.is_err());
    assert_eq!(
        connection
            .query_all_raw(shared_history.clone())
            .await?
            .len()
            + 1,
        before_down
    );
    enabled
        .app()?
        .run_command("migrate", vec!["up".to_owned(), "1".to_owned()])
        .await?;
    connection.query_one_raw(statement.clone()).await?;
    assert_eq!(
        connection.query_all_raw(shared_history).await?.len(),
        before_down
    );
    enabled
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;

    let mut disabled = TestApp::<crate::bootstrap::Server>::builder(sources());
    disabled.remove_module("example");
    let mut disabled = disabled.build().await?;
    disabled.app()?.run_command("migrate", Vec::new()).await?;
    connection.query_one_raw(statement).await?;
    disabled
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}
