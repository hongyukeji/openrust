//! 验证组装后 server 的集成边界。

#[path = "../../src/app.rs"]
mod app;
#[path = "../../src/routes.rs"]
mod routes;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use figment::Figment;
#[cfg(any(feature = "db-postgres", feature = "db-mysql", feature = "cache-redis"))]
use figment::providers::Format;
use openrust::ServiceExt;
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use openrust::config::FrameworkConfig;
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::build_router;
use openrust::prelude::{AppBuilder, AppBuilderHttpExt, Application, Json};
#[cfg(unix)]
use openrust::tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use openrust::tokio::net::TcpStream;
use openrust::{config, lifecycle};
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use sea_orm::ConnectionTrait;
#[cfg(unix)]
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::time::Duration;

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

#[cfg(feature = "cache-redis")]
#[test]
fn cache_plugin_uses_the_configured_shared_redis_store() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let app = test_server().await?;
        let cache = app.state().try_get::<openrust::cache::Cache>()?;
        let store = cache.default_store()?;
        assert_eq!(store.scope(), openrust::cache::CacheScope::Shared);
        let key = format!("server-test-{}", std::process::id());
        cache
            .set(&key, &"connected", Some(Duration::from_secs(10)))
            .await?;
        assert_eq!(
            cache.get::<String>(&key).await?.as_deref(),
            Some("connected")
        );
        cache.remove(&key).await?;
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
    let loader = config::ConfigLoader::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/config"),
        config::Environment::Development,
    );
    let figment = loader.figment();
    #[cfg(feature = "cache-redis")]
    let figment = figment.merge(figment::providers::Toml::string(
        "[cache]\ndefault_store = 'default'\n[cache.stores.default]\ndriver = 'redis'\nnamespace = 'server-test'\ndsn = 'redis://127.0.0.1:6379/'\n",
    ));
    #[cfg(feature = "encryption")]
    let figment = figment
        .merge(("security.encryption.active_key_id", "local"))
        .merge((
            "security.encryption.keys.local",
            "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
        ));
    let application_config = figment
        .extract::<<app::Server as Application>::Config>()
        .map_err(openrust::config::ConfigError::from)?;
    let mut builder = AppBuilder::new(loader.load_framework()?, figment);
    builder.insert(application_config);
    app::Server::build(&mut builder);
    lifecycle::finalize(builder).await
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

#[cfg(unix)]
#[test]
fn serve_process_exits_successfully_after_sigterm() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = reservation.local_addr()?.port();
        drop(reservation);
        let mut child = ChildGuard(
            Command::new(env!("CARGO_BIN_EXE_server"))
                .env("APP_SERVER__HOST", "127.0.0.1")
                .env("APP_SERVER__PORT", port.to_string())
                .env("APP_SERVER__SHUTDOWN_TIMEOUT_SECS", "2")
                .env("APP_SECURITY__ENCRYPTION__ACTIVE_KEY_ID", "test")
                .env(
                    "APP_SECURITY__ENCRYPTION__KEYS__TEST",
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                )
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let address = format!("127.0.0.1:{port}");
        openrust::tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if request_status(&address, "/ready")
                    .await
                    .is_ok_and(|status| status == 200)
                {
                    break;
                }
                openrust::tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await?;
        let signal = std::process::Command::new("kill")
            .args(["-TERM", &child.0.id().to_string()])
            .status()?;
        assert!(signal.success(), "could not send SIGTERM to server process");
        let status = openrust::tokio::time::timeout(Duration::from_secs(6), async {
            loop {
                if let Some(status) = child.0.try_wait()? {
                    return Ok::<_, std::io::Error>(status);
                }
                openrust::tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        assert!(status.success(), "serve command exited with {status}");
        Ok(())
    })
}

#[cfg(unix)]
async fn request_status(address: &str, path: &str) -> std::io::Result<u16> {
    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response = String::from_utf8_lossy(&response);
    response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse().ok())
        .ok_or_else(|| std::io::Error::other("server returned an invalid HTTP status line"))
}

#[cfg(unix)]
struct ChildGuard(Child);

#[cfg(unix)]
#[cfg(unix)]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            self.0.kill().ok();
            self.0.wait().ok();
        }
    }
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
        assert_eq!(json["data"]["name"], "server");
        assert_eq!(json["data"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["request_id"], request_id);
        Ok(())
    })
}

#[test]
fn resource_macro_registers_controller_actions() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let app = test_application::<ResourceMacroApp>().await?;
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

async fn test_application<A: Application>() -> Result<openrust::App, openrust::Error> {
    let loader = config::ConfigLoader::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/config"),
        config::Environment::Development,
    );
    let application_config = loader.load::<A::Config>()?;
    let mut builder = AppBuilder::new(loader.load_framework()?, loader.figment());
    builder.insert(application_config);
    A::build(&mut builder);
    lifecycle::finalize(builder).await
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
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_server"));
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
    assert!(stdout.contains("server"));
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
    assert_eq!(framework.app.name, "server");
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
