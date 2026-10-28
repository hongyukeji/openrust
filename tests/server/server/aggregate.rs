//! 验证组装后 server 的集成边界。

#[path = "../../../bootstrap/app.rs"]
mod apps;
use apps as bootstrap;

#[path = "../../../app/mod.rs"]
mod app;

#[path = "../../../routes/web.rs"]
mod routes;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use figment::Figment;
#[cfg(any(feature = "db-postgres", feature = "db-mysql", feature = "cache-redis"))]
use figment::providers::Format;
use openrust::ServiceExt;
use openrust::config;
use openrust::core::{BootContext, BoxFuture, ProviderBuilder, ServiceProvider};
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::build_router;
use openrust::prelude::{Application, Json};
#[cfg(unix)]
use openrust::tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use openrust::tokio::net::TcpStream;
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
use sea_orm::ConnectionTrait;
#[cfg(unix)]
use std::process::{Child, Command, Stdio};
#[cfg(any(unix, feature = "cache-redis"))]
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn mailer_plugin_is_registered_by_the_assembled_server() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let app = test_server().await?;
        app.state().resolve::<openrust::mail::Mailer>().await?;
        app.state().resolve::<openrust::sms::SmsSender>().await?;
        Ok(())
    })
}

#[cfg(feature = "cache-redis")]
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "一个真实 Redis 聚合场景同时检查基础操作、tag、singleflight 与 SWR"
)]
fn cache_plugin_uses_the_configured_shared_redis_store() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let app = test_server().await?;
        let cache = app.state().resolve::<openrust::cache::Cache>().await?;
        let readiness = app.state().resolve::<openrust::core::Readiness>().await?;
        assert!(readiness.dependencies_ready().await);
        let store = cache.default_store().await?;
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

        let tagged = store.tagged(&["server-integration"])?;
        tagged
            .set("profile", &"visible", Duration::from_secs(10))
            .await?;
        let stale_entry = tagged.entry("stale-profile").await?;
        assert_eq!(
            tagged.get::<String>("profile").await?.as_deref(),
            Some("visible")
        );
        store.invalidate_tags(&["server-integration"]).await?;
        assert_eq!(tagged.get::<String>("profile").await?, None);
        assert!(
            stale_entry
                .set(&"stale", Duration::from_secs(10))
                .await
                .is_err()
        );

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut loads = Vec::new();
        for _ in 0..4 {
            let cache = cache.clone();
            let calls = std::sync::Arc::clone(&calls);
            let load_key = format!("singleflight-{}", std::process::id());
            loads.push(openrust::tokio::spawn(async move {
                cache
                    .get_or_load_with(
                        &load_key,
                        "server-integration-v1",
                        Some(Duration::from_secs(10)),
                        || {
                            let calls = std::sync::Arc::clone(&calls);
                            async move {
                                calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                                openrust::tokio::time::sleep(Duration::from_millis(20)).await;
                                Ok("loaded".to_owned())
                            }
                        },
                    )
                    .await
            }));
        }
        for load in loads {
            assert_eq!(load.await??, "loaded");
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 1);

        let second_app = test_server().await?;
        let second_cache = second_app
            .state()
            .resolve::<openrust::cache::Cache>()
            .await?;
        let tagged_a = store.tagged(&["server-swr-integration"])?;
        let tagged_b = second_cache
            .default_store()
            .await?
            .tagged(&["server-swr-integration"])?;
        let swr_key = format!("profile-{}", std::process::id());
        let initial = tagged_a
            .get_stale_while_revalidate(
                &swr_key,
                "server-swr-v1",
                Duration::from_millis(30),
                Duration::from_secs(3),
                || async { Ok("initial".to_owned()) },
            )
            .await?;
        assert_eq!(initial, "initial");
        openrust::tokio::time::sleep(Duration::from_millis(40)).await;
        let stale = tagged_b
            .get_stale_while_revalidate(
                &swr_key,
                "server-swr-v1",
                Duration::from_millis(30),
                Duration::from_secs(3),
                || async { Ok("refreshed".to_owned()) },
            )
            .await?;
        assert_eq!(stale, "initial");
        let mut refreshed = stale;
        for _ in 0..100 {
            openrust::tokio::time::sleep(Duration::from_millis(5)).await;
            refreshed = tagged_a
                .get_stale_while_revalidate(
                    &swr_key,
                    "server-swr-v1",
                    Duration::from_millis(30),
                    Duration::from_secs(3),
                    || async { Ok("refreshed".to_owned()) },
                )
                .await?;
            if refreshed == "refreshed" {
                break;
            }
        }
        assert_eq!(refreshed, "refreshed");
        Ok(())
    })
}

struct ResourceMacroApp;

impl Application for ResourceMacroApp {
    type Config = openrust::NoConfig;

    fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        vec![
            Box::new(openrust::http::HttpServiceProvider),
            Box::new(MacroRoutesProvider),
        ]
    }
}

struct MacroRoutesProvider;

impl ServiceProvider for MacroRoutesProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes| routes.resource::<MacroPosts>("/api/posts"))?;
            Ok(())
        })
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
    build_router(&app).inspect_err(|error| {
        eprintln!("server fixture router stage failed: {}", error.code());
    })
}

async fn test_server() -> Result<openrust::App, openrust::Error> {
    let loader = config::ConfigLoader::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/config"),
        config::Environment::Development,
    );
    let figment = loader.figment().inspect_err(|_| {
        eprintln!("server fixture config loader stage failed");
    })?;
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
        .extract::<<apps::Server as Application>::Config>()
        .map_err(openrust::config::ConfigError::from)?;
    let mut builder = ProviderBuilder::new();
    builder.app_config::<<apps::Server as Application>::Config>();
    for provider in apps::Server::providers(&application_config) {
        builder.boxed_provider(provider);
    }
    let runtime = builder.start(figment).await.inspect_err(|error| {
        eprintln!("server fixture provider stage failed: {}", error.code());
    })?;
    openrust::App::from_provider_runtime(runtime).inspect_err(|error| {
        eprintln!("server fixture app stage failed: {}", error.code());
    })
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
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
        assert_eq!(body["success"], true);
        assert_eq!(body["code"], "ok");
        assert_eq!(body["message"], "ok");
        assert_eq!(body["data"], serde_json::json!({"status": "ok"}));
        assert!(body.get("request_id").is_none());
        assert!(body.get("timestamp").is_none());
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
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .env("APP_SERVER_HOST", "127.0.0.1")
                .env("APP_SERVER_PORT", port.to_string())
                .env("APP_SERVER_SHUTDOWN_TIMEOUT_SECS", "2")
                .env("APP_SECURITY_ENCRYPTION_ACTIVE_KEY_ID", "test")
                .env(
                    "APP_SECURITY_ENCRYPTION_KEYS_TEST",
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
        let homepage = request_bytes(&address, "/").await?;
        assert!(String::from_utf8_lossy(&homepage).contains("<title>OpenRust</title>"));
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
    let response = request_bytes(address, path).await?;
    let response = String::from_utf8_lossy(&response);
    response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse().ok())
        .ok_or_else(|| std::io::Error::other("server returned an invalid HTTP status line"))
}

#[cfg(unix)]
async fn request_bytes(address: &str, path: &str) -> std::io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    Ok(response)
}

#[cfg(all(unix, feature = "metrics"))]
#[test]
fn metrics_scrape_is_served_over_tcp_only_when_explicitly_enabled() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = reservation.local_addr()?.port();
        drop(reservation);
        let mut child = ChildGuard(
            Command::new(env!("CARGO_BIN_EXE_server"))
                .env("APP_SERVER_HOST", "127.0.0.1")
                .env("APP_SERVER_PORT", port.to_string())
                .env("APP_SERVER_METRICS_ENDPOINT_ENABLED", "true")
                .env("APP_SECURITY_ENCRYPTION_ACTIVE_KEY_ID", "test")
                .env(
                    "APP_SECURITY_ENCRYPTION_KEYS_TEST",
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
        let _ = request_bytes(&address, "/").await?;
        let scrape = request_bytes(&address, "/metrics").await?;
        let scrape = String::from_utf8(scrape)?;
        let (headers, body) = scrape
            .split_once("\r\n\r\n")
            .ok_or("metrics response did not contain an HTTP header boundary")?;
        assert!(headers.starts_with("HTTP/1.1 200"));
        assert!(headers.to_ascii_lowercase().contains("text/plain"));
        assert!(body.contains("openrust_http_requests_total"));
        assert!(!body.contains("/metrics"));
        assert!(!body.contains("x-request-id"));
        let signal = Command::new("kill")
            .args(["-TERM", &child.0.id().to_string()])
            .status()?;
        assert!(signal.success(), "could not stop metrics test server");
        let status = openrust::tokio::time::timeout(Duration::from_secs(6), async {
            loop {
                if let Some(status) = child.0.try_wait()? {
                    return Ok::<_, std::io::Error>(status);
                }
                openrust::tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        assert!(status.success(), "metrics test server exited with {status}");
        Ok(())
    })
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
fn api_root_reports_server_name_and_version() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let response = router()
            .await?
            .oneshot(Request::get("/api").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(json["success"], true);
        assert_eq!(json["code"], "ok");
        assert_eq!(json["data"]["name"], "openrust");
        assert_eq!(json["data"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(json.get("request_id").is_none());
        assert!(json.get("timestamp").is_none());
        Ok(())
    })
}

#[test]
fn example_business_route_is_mounted_through_the_root_application() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let response = router()
            .await?
            .oneshot(Request::get("/api/example").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(json["data"]["message"], "Hello from app/example");
        Ok(())
    })
}

#[test]
fn database_configuration_enables_infrastructure_without_silent_fallback() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let loader = config::ConfigLoader::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/config"),
            config::Environment::Development,
        );
        let figment = loader
            .figment()?
            .merge(("database.url", "unsupported://configured-but-invalid"))
            .merge(("security.encryption.active_key_id", "local"))
            .merge((
                "security.encryption.keys.local",
                "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
            ));
        let configuration = figment.extract::<apps::Configuration>()?;
        let mut builder = ProviderBuilder::new();
        builder.app_config::<apps::Configuration>();
        for provider in apps::Server::providers(&configuration) {
            builder.boxed_provider(provider);
        }
        let error = builder
            .start(figment)
            .await
            .err()
            .ok_or("声明错误数据库应导致启动失败")?;
        assert_eq!(error.code(), "configuration_error");
        assert!(!error.to_string().contains("configured-but-invalid"));
        Ok(())
    })
}

#[test]
fn root_serves_the_default_public_index() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let response = router()
            .await?
            .oneshot(Request::get("/").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "text/html"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        assert!(std::str::from_utf8(&body)?.contains("OpenRust"));
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
    let mut builder = ProviderBuilder::new();
    builder.app_config::<A::Config>();
    for provider in A::providers(&application_config) {
        builder.boxed_provider(provider);
    }
    openrust::App::from_provider_runtime(builder.start(loader.figment()?).await?)
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
        let mut builder = ProviderBuilder::new();
        builder.provider(openrust::db::DatabaseServiceProvider);
        let app = openrust::App::from_provider_runtime(builder.start(figment).await?)?;
        let database = app.state().resolve::<openrust::Db>().await?.connection()?;
        database.execute_unprepared("SELECT 1").await?;
        app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        Ok(())
    })
}

#[test]
fn config_show_runs_and_redacts_secrets() -> TestResult {
    let selected = tempfile::Builder::new()
        .prefix("SECRET_CONFIG_DIRECTORY_SENTINEL")
        .tempdir()?;
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/config"))? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            let name = path.file_name().ok_or("配置文件名缺失")?;
            std::fs::copy(&path, selected.path().join(name))?;
        }
    }
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_server"));
    command
        .args(["config", "show"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("APP_API_TOKEN", "server-secret-value")
        .env("APP_CONFIG_DIR", selected.path())
        .env("APP_LOG_LEVEL", "debug");
    #[cfg(feature = "encryption")]
    command
        .env("APP_SECURITY_ENCRYPTION_ACTIVE_KEY_ID", "local")
        .env(
            "APP_SECURITY_ENCRYPTION_KEYS_LOCAL",
            "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
        );
    let output = command.output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(!stdout.contains("SECRET_CONFIG_DIRECTORY_SENTINEL"));
    assert!(!stderr.contains("SECRET_CONFIG_DIRECTORY_SENTINEL"));
    assert!(stdout.contains("server"));
    assert!(stdout.contains("********"), "{stdout}");
    assert!(!stdout.contains("server-secret-value"));
    Ok(())
}

#[test]
fn configured_missing_directory_fails_before_application_boot() -> TestResult {
    let missing = std::env::temp_dir().join(format!(
        "openrust-SECRET_CONFIG_DIRECTORY_SENTINEL-{}",
        std::process::id()
    ));
    assert!(!missing.exists());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_server"))
        .args(["config", "show"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("APP_CONFIG_DIR", missing)
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("APP_CONFIG_DIR"));
    assert!(!stderr.contains("SECRET_CONFIG_DIRECTORY_SENTINEL"));
    assert!(!stderr.contains("panicked"));
    Ok(())
}

#[test]
fn default_config_loads_from_package_directory() -> TestResult {
    let loader = config::ConfigLoader::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/config"),
        config::Environment::Development,
    );
    let framework = loader.load_framework()?;
    assert_eq!(framework.app.name, "openrust");
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
        assert!(json["timestamp"].as_str().is_some_and(|value| {
            value.ends_with('Z') && chrono::DateTime::parse_from_rfc3339(value).is_ok()
        }));
        Ok(())
    })
}
