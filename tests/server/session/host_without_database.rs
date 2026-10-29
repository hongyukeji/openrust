//! 正式 Server 在无数据库、无 Session Provider 时不暴露示例 Session 路由。

use super::Server;
use figment::{Figment, providers::Serialized};
use openrust::config::{FrameworkConfig, HttpStatusMode};
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

#[test]
fn database_host_registers_one_session_provider_even_without_auth() {
    let mut config = super::Configuration::default();
    config.database.url = Some("postgres://placeholder.invalid/not_connected".into());
    let session = ProviderId::of::<SessionServiceProvider>();
    let count = Server::providers(&config)
        .iter()
        .filter(|provider| provider.id() == session)
        .count();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn no_database_host_keeps_example_but_omits_session_route() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
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
        "modules_root": root.path(),
        "app": {"env": "test"},
        "cache": {"default_store": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {"active_key_id": "test", "keys": {
            "test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
        }}}
    })));
    let mut app = TestApp::<Server>::builder(source).build().await?;
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
    fn providers(config: &Self::Config) -> Vec<Box<dyn openrust::core::ServiceProvider>> {
        let mut providers = Server::providers(&config.server);
        providers.push(Box::new(SessionServiceProvider::with_store(
            openrust::session::MemorySessionStore::default(),
        )));
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        let root = config.server.application_root();
        Server::modules(&config.server).module(
            openrust::core::ModuleDescriptor::new(
                "session_late_overlay",
                root.join("modules/session_late_overlay"),
                1,
            )
            .requires("example")
            .provider(LateDriver(config.late_driver.clone())),
        )
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
        for directory in [
            "modules/example/config",
            "modules/example/lang",
            "modules/example/resources/views",
            "modules/session_late_overlay",
        ] {
            std::fs::create_dir_all(root.path().join(directory)).map_err(Error::internal)?;
        }
        std::fs::write(
            root.path().join("modules_statuses.json"),
            r#"{"example":true,"session_late_overlay":true}"#,
        )
        .map_err(Error::internal)?;
        let mut config = FrameworkConfig::default();
        config.api.http_status = HttpStatusMode::Semantic;
        let source = Figment::from(Serialized::defaults(config)).merge(Serialized::defaults(json!({
            "modules_root":root.path(), "late_driver":final_driver,
            "app":{"env":"test"}, "session":{"driver":initial,"secure":false},
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })));
        let mut app =
            tokio::time::timeout(TIMEOUT, TestApp::<LateDriverApp>::builder(source).build())
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
