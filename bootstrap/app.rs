//! 默认应用的框架与业务模块 Provider 清单。

use openrust::core::{BootContext, BoxFuture, RegisterContext, ServiceProvider};
use openrust::prelude::*;
use std::sync::Arc;

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

struct ReadinessProvider;
impl ServiceProvider for ReadinessProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(openrust::core::Readiness::default()));
        Ok(())
    }
}

struct FrameworkRoutesProvider;
impl ServiceProvider for FrameworkRoutesProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(crate::routes::register)?;
            Ok(())
        })
    }
}

/// 框架与综合示例共用的默认应用宿主。
pub(crate) struct Server;
impl Application for Server {
    type Config = Configuration;

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers: Vec<Box<dyn ServiceProvider>> = vec![
            Box::new(openrust::http::HttpServiceProvider),
            Box::new(ReadinessProvider),
            Box::new(openrust::i18n::I18nServiceProvider::default()),
            Box::new(openrust::mail::MailerServiceProvider),
            Box::new(openrust::sms::SmsServiceProvider),
        ];
        providers.push(Box::new(openrust::cache::CacheServiceProvider::new()));
        #[cfg(feature = "encryption")]
        providers.push(Box::new(openrust::security::EncryptionServiceProvider));
        #[cfg(feature = "storage-local")]
        providers.push(Box::new(openrust::storage::StorageServiceProvider::new()));
        #[cfg(feature = "view")]
        providers.push(Box::new(openrust::view::OptionalViewServiceProvider));
        providers.push(Box::new(openrust::db::DatabaseServiceProvider));
        if config.uses_database() {
            #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
            providers.push(Box::new(openrust::db::MigrationServiceProvider::<
                openrust::db::FrameworkMigrator,
            >::new()));
            #[cfg(feature = "auth")]
            {
                providers.push(Box::new(openrust::auth::JwtServiceProvider));
                providers.push(Box::new(openrust::auth::AuthServiceProvider::new()));
                providers.push(Box::new(openrust::audit::AuditServiceProvider::database()));
                providers.push(Box::new(openrust::auth::AuthFlowsServiceProvider::new()));
                providers.push(Box::new(openrust::auth::SessionServiceProvider::new()));
                #[cfg(feature = "auth-mfa")]
                providers.push(Box::new(openrust::auth::MfaServiceProvider));
            }
            #[cfg(feature = "signing")]
            providers.push(Box::new(
                openrust::signing::SignedLinksServiceProvider::new()
                    .with_database_revocations_from_app(),
            ));
            #[cfg(feature = "queue")]
            {
                let queue = openrust::queue::QueueServiceProvider::new();
                providers.push(Box::new(queue));
                providers.push(Box::new(
                    openrust::notifications::NotificationsServiceProvider,
                ));
            }
        }
        providers.push(Box::new(FrameworkRoutesProvider));
        providers
    }

    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        let root = config.application_root();
        openrust::core::ModuleManifest::from_file(root.join("modules_statuses.json")).module(
            crate::modules::example::descriptor(config.uses_database(), root),
        )
    }
}

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
            "cache": {"default_store": "default", "stores": {"default": {"driver": "memory"}}},
            "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })));
        let runtime = builder.start(source).await?;
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
        std::fs::create_dir_all(module.join("config")).map_err(openrust::Error::internal)?;
        std::fs::create_dir_all(module.join("resources/views"))
            .map_err(openrust::Error::internal)?;
        std::fs::create_dir_all(deployed.path().join("resources/lang/vendor/example"))
            .map_err(openrust::Error::internal)?;
        std::fs::write(
            module.join("config/config.toml"),
            "greeting = 'Relocated module'\n",
        )
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
                "cache": {"default_store": "default", "stores": {"default": {"driver": "memory"}}},
                "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
            })));
        let mut app = openrust::testing::TestApp::<Server>::builder(source)
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
        assert_eq!(json["data"]["message"], "Relocated module");
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
    }
}
