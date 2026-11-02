use openrust::core::ServiceProvider;
use openrust::prelude::*;

mod providers;

/// 应用运行根锚点与独立的模块激活配置；默认从当前工作目录解析。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct Configuration {
    pub database: openrust::config::DatabaseConfig,
    pub modules: openrust::config::ModulesConfig,
    pub application_root: Option<std::path::PathBuf>,
}

pub struct {{type_name}};

impl Application for {{type_name}} {
    type Config = Configuration;

    fn config() -> openrust::config::ConfigManifest {
        crate::config::boot()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        providers::application_providers(config)
    }

    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        crate::modules::manifest(
            config
                .application_root
                .as_deref()
                .unwrap_or(std::path::Path::new(".")),
            &config.modules,
        )
    }
}

/// 在启动装配中指定 API 前缀与无状态组，再登记应用 Web 路由和全局 fallback。
pub(crate) fn routes(routes: Routes, prefix: &str) -> Routes {
    let routes = routes.group(
        RouteGroup::new().prefix(prefix).middleware_group("api"),
        crate::routes::api::register,
    );
    crate::routes::web::register(routes)
}
