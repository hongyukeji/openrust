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
                "cache": {"default_store": "default", "stores": {"default": {"driver": "memory"}}}
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
        bindings.instance(Arc::new(super::services::ExampleMessage::new(
            "<script>alert('x' & \"y\")</script>",
        )));
        Ok(())
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
#[tokio::test]
#[allow(clippy::too_many_lines)] // 同一隔离库验证启用、迁移与禁用后历史留存。
async fn module_migration_runs_with_framework_and_disable_keeps_existing_table()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::database::sea_orm::{ConnectionTrait, Statement};
    use openrust::testing::{TestApp, TestDatabase};
    use std::time::Duration;

    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping module migration test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let sources = || module_migration_sources(&database);
    let mut enabled = TestApp::<crate::bootstrap::Server>::builder(sources())
        .build()
        .await?;
    enabled.app()?.run_command("migrate", Vec::new()).await?;
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
    let before_down = connection
        .query_all_raw(shared_history.clone())
        .await?
        .len();
    enabled
        .app()?
        .run_command("migrate", vec!["down".to_owned(), "1".to_owned()])
        .await?;
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
        before_down
    );
    enabled
        .app()?
        .run_command("migrate", vec!["up".to_owned(), "1".to_owned()])
        .await?;
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
