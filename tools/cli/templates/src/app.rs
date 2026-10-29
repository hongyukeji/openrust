use openrust::core::{BootContext, BoxFuture, RegisterContext, ServiceProvider};
use openrust::prelude::*;
use std::sync::Arc;

use crate::routes;

struct ReadinessProvider;
impl ServiceProvider for ReadinessProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(openrust::core::Readiness::default()));
        Ok(())
    }
}

struct RoutesProvider;
impl ServiceProvider for RoutesProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(routes::register)?;
            Ok(())
        })
    }
}

/// 应用自己的运行时模块根；默认从当前工作目录解析。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct Configuration {
    pub database: openrust::config::DatabaseConfig,
    pub session: openrust::config::SessionConfig,
    pub modules_root: Option<std::path::PathBuf>,
}

pub struct {{type_name}};

impl Application for {{type_name}} {
    type Config = Configuration;

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers: Vec<Box<dyn ServiceProvider>> = vec![
            Box::new(openrust::http::HttpServiceProvider),
            Box::new(ReadinessProvider),
            Box::new(openrust::view::ViewServiceProvider),
            Box::new(openrust::i18n::I18nServiceProvider::default()),
            Box::new(openrust::cache::CacheServiceProvider::new()),
            Box::new(openrust::mail::MailerServiceProvider),
            Box::new(openrust::sms::SmsServiceProvider),
        ];{{database_providers}}{{encryption_provider}}{{cookie_provider}}{{grpc_providers}}
        let has_database = config.database.url.is_some()
            || !config.database.instances.is_empty()
            || !config.database.read_replicas.is_empty();
        if has_database
            || config
                .session
                .driver
                .as_deref()
                .is_some_and(|driver| driver != "database")
        {
            providers.push(Box::new(openrust::session::SessionServiceProvider::new()));
        }
        providers.push(Box::new(RoutesProvider));
        providers
    }

    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        crate::modules::manifest(
            config
                .modules_root
                .as_deref()
                .unwrap_or(std::path::Path::new(".")),
        )
    }
}
