//! 默认应用的框架与业务模块 Provider 清单。

use openrust::core::ServiceProvider;
use openrust::prelude::*;

mod providers;

/// 启动来源决定需要数据库的应用能力；后续配置覆盖由各 Provider 验证。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Configuration {
    database: openrust::config::DatabaseConfig,
    pub modules: openrust::config::ModulesConfig,
    // 运行时应用根锚点，供所有模块资源解析；不再兼作状态文件配置。
    application_root: Option<std::path::PathBuf>,
}

impl Configuration {
    fn application_root(&self) -> std::path::PathBuf {
        self.application_root
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
        crate::modules::example::manifest(config.uses_database(), &root, &config.modules)
    }
}

/// 装配应用 API 的统一前缀与无状态中间件组，再加载应用 Web 声明。
pub(crate) fn routes(routes: openrust::http::Routes) -> openrust::http::Routes {
    let routes = routes.group(
        openrust::http::RouteGroup::new()
            .prefix("/api")
            .middleware_group("api"),
        crate::routes::api::register,
    );
    crate::routes::web::register(routes)
}

#[cfg(all(test, feature = "session"))]
#[path = "../tests/server/session/host_without_database.rs"]
mod host_without_database;

// 私有宿主类型只通过测试子模块访问；生产入口不承载测试主体或扩大可见性。
#[cfg(test)]
#[path = "../tests/server/bootstrap.rs"]
mod tests;
