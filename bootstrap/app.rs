//! 默认应用的框架与业务模块 Provider 清单。

use openrust::core::ServiceProvider;
use openrust::prelude::*;

mod providers;

/// 启动来源决定需要数据库的应用能力；后续配置覆盖由各 Provider 验证。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Configuration {
    database: openrust::config::DatabaseConfig,
    modules_root: Option<std::path::PathBuf>,
}

impl Configuration {
    fn application_root(&self) -> std::path::PathBuf {
        self.modules_root
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    fn uses_database(&self) -> bool {
        self.database.url.is_some()
            || !self.database.instances.is_empty()
            || !self.database.read_replicas.is_empty()
    }
}

/// 框架与综合示例共用的默认应用宿主。
pub(crate) struct Server;
impl Application for Server {
    type Config = Configuration;

    fn config() -> openrust::config::ConfigManifest {
        crate::config::boot()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        providers::application_providers(config)
    }

    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        let root = config.application_root();
        openrust::core::ModuleManifest::from_file(root.join("modules_statuses.json")).module(
            crate::modules::example::descriptor(config.uses_database(), root),
        )
    }
}

#[cfg(all(test, feature = "session"))]
#[path = "../tests/server/session/host_without_database.rs"]
mod host_without_database;

#[cfg(test)]
mod tests {
    use super::*;
    use figment::{Figment, providers::Serialized};
    use openrust::ServiceExt;

    #[tokio::test]
    async fn default_provider_manifest_serves_framework_route() -> Result<()> {
        let builder =
            openrust::core::ProviderBuilder::for_application::<Server>(&Configuration::default());
        let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
            .merge(Serialized::defaults(serde_json::json!({
            "app": {"env": "test"},
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
            "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })));
        let runtime = builder
            .start(crate::modules::example::test_sources(&(source))?)
            .await?;
        let app = openrust::App::from_provider_runtime(runtime)?;
        let router = openrust::http::build_router(&app)?;
        let response = router
            .oneshot(
                openrust::http::axum::http::Request::builder()
                    .uri("/api")
                    .header("host", "localhost")
                    .body(openrust::http::axum::body::Body::empty())
                    .map_err(openrust::Error::internal)?,
            )
            .await
            .map_err(openrust::Error::internal)?;
        assert_eq!(
            response.status(),
            openrust::http::axum::http::StatusCode::OK
        );
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn module_resources_follow_configured_runtime_root() -> Result<()> {
        use openrust::http::axum::{body::Body, http::Request};

        let deployed = tempfile::tempdir().map_err(openrust::Error::internal)?;
        let module = deployed.path().join("modules/example");
        std::fs::create_dir_all(module.join("resources/views"))
            .map_err(openrust::Error::internal)?;
        std::fs::create_dir_all(deployed.path().join("resources/lang/vendor/example"))
            .map_err(openrust::Error::internal)?;
        std::fs::write(
            deployed.path().join("modules_statuses.json"),
            "{\"example\":true}\n",
        )
        .map_err(openrust::Error::internal)?;
        let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
            .merge(Serialized::defaults(serde_json::json!({
                "modules_root": deployed.path(),
                "app": {"env": "test"},
                "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
                "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
            })));
        let mut app = openrust::testing::TestApp::<Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        let response = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())
                    .map_err(openrust::Error::internal)?,
            )
            .await?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(json["data"]["message"], "Hello from modules/example");
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
    }
}
