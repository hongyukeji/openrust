//! 正式 Server 在无数据库、无 Session Provider 时不暴露示例 Session 路由。

use super::Server;
use figment::{Figment, providers::Serialized};
use openrust::config::{FrameworkConfig, HttpStatusMode, repository::ConfigRepository};
use openrust::core::{Application, ProviderId};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::session::{SessionServiceProvider, SessionStoreHandle};
use openrust::testing::TestApp;
use openrust::{Error, Result};
use serde_json::json;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);

fn selected_session_provider_count(
    config: &openrust::config::DatabaseConfig,
    driver: Option<&str>,
) -> Result<usize> {
    let sources = Figment::from(Serialized::defaults(json!({
        "database": {"url": &config.url},
        "session": {"driver": driver}
    })));
    let repository = ConfigRepository::new(sources);
    let defaults = Server::default_providers().resolve(&repository)?;
    let application = Server::providers(&super::Configuration::default());
    let session = ProviderId::of::<SessionServiceProvider>();
    Ok(defaults
        .iter()
        .chain(application.iter())
        .filter(|provider| provider.id() == session)
        .count())
}

#[test]
fn database_host_selects_one_session_provider_even_without_auth() -> Result<()> {
    let config = openrust::config::DatabaseConfig {
        url: Some("postgres://placeholder.invalid/not_connected".into()),
        ..openrust::config::DatabaseConfig::default()
    };
    assert_eq!(
        selected_session_provider_count(&config, Some("database"))?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn host_with_explicitly_disabled_session_keeps_example_but_omits_session_route() -> Result<()>
{
    let root = tempfile::tempdir().map_err(Error::internal)?;
    crate::modules::example::write_discovery_fixture(root.path())?;
    let module = root.path().join("modules/example");
    for directory in ["config", "lang", "resources/views"] {
        std::fs::create_dir_all(module.join(directory)).map_err(Error::internal)?;
    }
    std::fs::write(
        root.path().join("modules_statuses.json"),
        "{\"example\":true}",
    )
    .map_err(Error::internal)?;
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let source = Figment::from(Serialized::defaults(config)).merge(Serialized::defaults(json!({
        "application_root": root.path(),
        "app": {"env": "test"},
        "session": {"driver": null},
        "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {"active_key_id": "test", "keys": {
            "test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
        }}}
    })));
    let mut app = TestApp::<Server>::builder(crate::modules::example::test_sources(&source)?)
        .build()
        .await?;
    let outcome: Result<()> = async {
        if app
            .app()?
            .state()
            .container()
            .contains::<SessionStoreHandle>()
        {
            return Err(Error::custom(
                "test_session_host_unexpected",
                "无数据库宿主不应安装 Session store",
            ));
        }
        for (path, expected) in [
            ("/api/example", StatusCode::OK),
            ("/api/example/session", StatusCode::NOT_FOUND),
        ] {
            let request = Request::get(path)
                .header("host", "localhost")
                .body(Body::empty())
                .map_err(Error::internal)?;
            app.request_with_limits(request, 4096, TIMEOUT)
                .await?
                .assert_status(expected)?;
        }
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown
}

#[derive(serde::Deserialize)]
struct LateDriverConfig {
    #[serde(flatten)]
    server: super::Configuration,
    late_driver: Option<String>,
}
struct LateDriverApp;
impl Application for LateDriverApp {
    type Config = LateDriverConfig;

    fn default_providers() -> openrust::core::DefaultProviders {
        Server::default_providers().except([ProviderId::of::<SessionServiceProvider>()])
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn openrust::core::ServiceProvider>> {
        let mut providers = Server::providers(&config.server);
        let session = Box::new(SessionServiceProvider::with_store(
            openrust::session::MemorySessionStore::default(),
        ));
        if let Some(index) = providers
            .iter()
            .position(|provider| provider.id() == ProviderId::of::<SessionServiceProvider>())
        {
            providers[index] = session;
        } else {
            providers.push(session);
        }
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        let driver = config.late_driver.clone();
        Server::modules(&config.server)
            .compiled_provider::<LateDriver, _>("LateDriverServiceProvider", move |_| {
                Ok(LateDriver(driver.clone()))
            })
            .possible_providers([ProviderId::of::<LateDriver>()])
    }
}
struct LateDriver(Option<String>);
impl openrust::core::ServiceProvider for LateDriver {
    fn dependencies(&self) -> Vec<ProviderId> {
        // 若真实清单未把Example放在前面，启动应失败，不能让测试靠早覆盖误过。
        vec![ProviderId::of::<
            crate::modules::example::providers::ExampleServiceProvider,
        >()]
    }
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.overlay("session", json!({"driver":self.0})) })
    }
}

#[tokio::test]
async fn late_driver_overlay_controls_actual_example_session_route() -> Result<()> {
    for (initial, final_driver, expected) in [
        (None, Some("array"), StatusCode::OK),
        (Some("array"), None, StatusCode::NOT_FOUND),
    ] {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        crate::modules::example::write_discovery_fixture(root.path())?;
        for directory in [
            "modules/example/config",
            "modules/example/lang",
            "modules/example/resources/views",
            "modules/session_late_overlay",
        ] {
            std::fs::create_dir_all(root.path().join(directory)).map_err(Error::internal)?;
        }
        std::fs::write(
            root.path().join("modules/session_late_overlay/module.json"),
            r#"{"name":"session_late_overlay","priority":1,"providers":["LateDriverServiceProvider"],"files":[]}"#,
        ).map_err(Error::internal)?;
        std::fs::write(
            root.path().join("modules_statuses.json"),
            r#"{"example":true,"session_late_overlay":true}"#,
        )
        .map_err(Error::internal)?;
        let mut config = FrameworkConfig::default();
        config.api.http_status = HttpStatusMode::Semantic;
        let source = Figment::from(Serialized::defaults(config)).merge(Serialized::defaults(json!({
            "application_root":root.path(), "late_driver":final_driver,
            "app":{"env":"test"}, "session":{"driver":initial,"secure":false},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })));
        let mut app = tokio::time::timeout(
            TIMEOUT,
            TestApp::<LateDriverApp>::builder(crate::modules::example::test_sources(&source)?)
                .build(),
        )
        .await
        .map_err(Error::internal)??;
        let outcome: Result<()> = async {
            let response = app
                .request_with_limits(
                    Request::get("/api/example/session")
                        .header("host", "localhost")
                        .body(Body::empty())
                        .map_err(Error::internal)?,
                    4096,
                    TIMEOUT,
                )
                .await?;
            response.assert_status(expected)?;
            if response.headers().contains_key("set-cookie") != final_driver.is_some() {
                return Err(Error::custom(
                    "test_session_route_late_config",
                    "最终Session状态与路由Cookie不一致",
                ));
            }
            Ok(())
        }
        .await;
        let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
        outcome?;
        shutdown?;
    }
    Ok(())
}

#[test]
fn explicit_non_database_driver_selects_once_without_database() -> Result<()> {
    for driver in [Some("cookie"), Some("array"), Some("unknown"), None] {
        let config = openrust::config::DatabaseConfig::default();
        let count = selected_session_provider_count(&config, driver)?;
        assert_eq!(count, usize::from(driver.is_some()));
    }
    Ok(())
}

#[tokio::test]
async fn explicit_cookie_without_database_uses_original_host_provider_and_persists_both_cookies()
-> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    crate::modules::example::write_discovery_fixture(root.path())?;
    for directory in ["config", "lang", "resources/views"] {
        std::fs::create_dir_all(root.path().join("modules/example").join(directory))
            .map_err(Error::internal)?;
    }
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"example":true}"#,
    )
    .map_err(Error::internal)?;
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let source = Figment::from(Serialized::defaults(config)).merge(Serialized::defaults(json!({
        "application_root":root.path(), "app":{"env":"test"},
        "session":{"driver":"cookie","cookie":"host_cookie_session","secure":false},
        "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
    })));
    let mut app = TestApp::<Server>::builder(crate::modules::example::test_sources(&source)?)
        .build()
        .await?;
    let outcome = cookie_host_roundtrip(&mut app).await;
    let stopped = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    stopped
}
async fn cookie_host_roundtrip(app: &mut TestApp<Server>) -> Result<()> {
    let response = app
        .request_with_limits(
            Request::post("/api/example/session")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action":"put","value":"host-persisted"}"#))
                .map_err(Error::internal)?,
            4096,
            TIMEOUT,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let pairs = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|header| {
            header.to_str().map_err(Error::internal).and_then(|text| {
                text.split(';')
                    .next()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::custom("test_cookie_missing", "缺少Cookie首段"))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    assert!(
        pairs
            .iter()
            .any(|pair| pair.starts_with("host_cookie_session="))
    );
    assert!(pairs.iter().any(|pair| {
        pair.split_once('=')
            .is_some_and(|(name, _)| openrust::session::SessionId::parse(name).is_ok())
    }));
    for _ in 0..2 {
        let loaded = app
            .request_with_limits(
                Request::get("/api/example/session")
                    .header("host", "localhost")
                    .header("cookie", pairs.join("; "))
                    .body(Body::empty())
                    .map_err(Error::internal)?,
                4096,
                TIMEOUT,
            )
            .await?;
        loaded.assert_status(StatusCode::OK)?;
        let body: serde_json::Value = loaded.json()?;
        assert_eq!(body["success"], true);
        assert_eq!(body["code"], "ok");
        assert_eq!(body["data"]["preference"], "host-persisted");
    }
    Ok(())
}
