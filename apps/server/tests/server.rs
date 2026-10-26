//! Integration boundaries of the assembled server.

#[path = "../src/app.rs"]
mod app;
#[path = "../src/routes.rs"]
mod routes;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use figment::Figment;
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use figment::providers::Format;
use openrust::ServiceExt;
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use openrust::config::FrameworkConfig;
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::build_router;
use openrust::prelude::{AppBuilder, AppBuilderHttpExt, Application, Json};
use openrust::{config, lifecycle};
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use sea_orm::ConnectionTrait;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn mailer_plugin_is_registered_by_the_assembled_server() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let app = test_server().await?;
        app.state().try_get::<openrust::mail::Mailer>()?;
        app.state().try_get::<openrust::sms::SmsSender>()?;
        Ok(())
    })
}

struct ResourceMacroApp;

impl Application for ResourceMacroApp {
    type Config = openrust::NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(openrust::http::HttpPlugin)
            .routes(|routes| routes.resource::<MacroPosts>("/api/posts"));
    }
}

struct MacroPosts;

#[openrust::resource]
impl MacroPosts {
    async fn list() -> Json<&'static str> {
        openrust::tokio::task::yield_now().await;
        Json("list")
    }

    async fn get() -> Json<&'static str> {
        openrust::tokio::task::yield_now().await;
        Json("get")
    }

    async fn create() -> Json<&'static str> {
        openrust::tokio::task::yield_now().await;
        Json("create")
    }

    async fn update() -> Json<&'static str> {
        openrust::tokio::task::yield_now().await;
        Json("update")
    }

    async fn delete() -> Json<&'static str> {
        openrust::tokio::task::yield_now().await;
        Json("delete")
    }
}

async fn router() -> Result<openrust::http::axum::Router, openrust::Error> {
    let app = test_server().await?;
    build_router(&app)
}

async fn test_server() -> Result<openrust::App, openrust::Error> {
    #[cfg(feature = "encryption")]
    {
        use openrust::config::{ConfigLoader, Environment};

        let loader = ConfigLoader::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/config"),
            Environment::Development,
        );
        let figment = loader
            .figment()
            .merge(("security.encryption.active_key_id", "local"))
            .merge((
                "security.encryption.keys.local",
                "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
            ));
        let mut builder = AppBuilder::new(loader.load_framework()?, figment);
        app::Server::build(&mut builder);
        lifecycle::finalize(builder).await
    }
    #[cfg(not(feature = "encryption"))]
    lifecycle::bootstrap::<app::Server>().await
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
fn resource_macro_registers_controller_actions() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let app = lifecycle::bootstrap::<ResourceMacroApp>().await?;
        let router = build_router(&app)?;
        for (method, uri, expected) in [
            ("GET", "/api/posts", "list"),
            ("GET", "/api/posts/42", "get"),
            ("POST", "/api/posts", "create"),
            ("PUT", "/api/posts/42", "update"),
            ("PATCH", "/api/posts/42", "update"),
            ("DELETE", "/api/posts/42", "delete"),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK, "{method} {uri}");
            let body = to_bytes(response.into_body(), usize::MAX).await?;
            let json: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(json["success"], true, "{method} {uri}");
            assert_eq!(json["data"], expected, "{method} {uri}");
        }
        Ok(())
    })
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[test]
fn database_plugin_is_available_through_the_facade() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping database connection: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let source = format!("[database]\nurl = {url:?}\n");
        let figment = Figment::new().merge(figment::providers::Toml::string(&source));
        let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
        builder.plugin(openrust::http::HttpPlugin);
        builder.plugin(openrust::DatabasePlugin);

        let app = lifecycle::finalize(builder).await?;
        let database = app.state().try_get::<openrust::Db>()?.connection()?;
        database.execute_unprepared("SELECT 1").await?;
        Ok(())
    })
}

#[test]
fn config_show_runs_and_redacts_secrets() -> TestResult {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_openrust-server"));
    command
        .args(["config", "show"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("APP_API__TOKEN", "server-secret-value");
    #[cfg(feature = "encryption")]
    command
        .env("APP_SECURITY__ENCRYPTION__ACTIVE_KEY_ID", "local")
        .env(
            "APP_SECURITY__ENCRYPTION__KEYS__LOCAL",
            "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
        );
    let output = command.output()?;
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
