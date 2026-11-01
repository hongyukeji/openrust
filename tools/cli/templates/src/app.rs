use openrust::core::ServiceProvider;
use openrust::prelude::*;

mod providers;

/// 应用自己的运行时模块根；默认从当前工作目录解析。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct Configuration {
    pub database: openrust::config::DatabaseConfig,
    pub modules_root: Option<std::path::PathBuf>,
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
                .modules_root
                .as_deref()
                .unwrap_or(std::path::Path::new(".")),
        )
    }
}

/// 在启动装配中指定 API 前缀与无状态组，再登记应用 Web 路由和全局 fallback。
pub(crate) fn routes(routes: Routes) -> Routes {
    let routes = routes.group(
        RouteGroup::new().prefix("/api").middleware_group("api"),
        crate::routes::api::register,
    );
    crate::routes::web::register(routes)
}
