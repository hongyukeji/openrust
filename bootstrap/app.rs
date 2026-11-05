//! 默认应用的框架与业务模块 Provider 清单。

use openrust::core::ServiceProvider;
use openrust::prelude::*;

mod providers;

/// 应用运行根与模块配置；数据库由统一配置仓储及其 Provider 读取。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Configuration {
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
}

/// 框架与综合示例共用的默认应用宿主。
pub(crate) struct Server;
impl Application for Server {
    type Config = Configuration;

    fn config() -> openrust::config::ConfigManifest {
        crate::config::boot()
    }

    fn default_providers() -> openrust::core::DefaultProviders {
        let providers = openrust::core::DefaultProviders::auto();
        #[cfg(feature = "auth-fortify")]
        let providers = providers.replace(
            openrust::core::ProviderId::of::<openrust::auth::fortify::FortifyServiceProvider>(),
            Box::new(crate::app::providers::AuthenticationProfileProvider::default()),
        );
        providers
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        providers::application_providers(config)
    }

    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        let root = config.application_root();
        crate::modules::manifest(&root, &config.modules)
    }
}

/// 装配应用 API 的统一前缀与无状态中间件组，再加载应用 Web 声明。
pub(crate) fn routes(routes: openrust::http::Routes, prefix: &str) -> openrust::http::Routes {
    let routes = routes.group(
        openrust::http::RouteGroup::new()
            .prefix(prefix)
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

#[cfg(all(
    test,
    feature = "auth-fortify",
    any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres")
))]
#[path = "../tests/server/auth/fortify.rs"]
mod fortify_flows;

#[cfg(all(
    test,
    feature = "auth-fortify",
    any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres")
))]
#[path = "../tests/server/auth/jwt_api.rs"]
mod jwt_api_flows;
