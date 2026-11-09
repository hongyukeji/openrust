//! 默认应用的框架与业务模块 Provider 清单。

use openrust::prelude::*;
use openrust::support::ServiceProvider;
use openrust_modules::ModuleProviderBuilderExt as _;

mod providers;
pub(crate) mod packages {
    include!(concat!(env!("OUT_DIR"), "/openrust_package_providers.rs"));
}

/// 应用运行根与模块配置；数据库由统一配置仓储及其 Provider 读取。
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Configuration {
    pub modules: openrust_modules::config::ModulesConfig,
    // 运行时应用根锚点，供所有模块资源解析；不再兼作状态文件配置。
    application_root: Option<std::path::PathBuf>,
}

impl Configuration {
    pub(crate) fn application_root(&self) -> std::path::PathBuf {
        self.application_root
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }
}

/// 框架与综合示例共用的默认应用宿主。
pub(crate) struct Server;
impl ApplicationDefinition for Server {
    type Config = Configuration;

    fn config() -> openrust::config::ConfigManifest {
        crate::config::boot()
    }

    fn default_providers() -> openrust::support::DefaultProviders {
        let providers = openrust::support::DefaultProviders::auto();
        #[cfg(feature = "auth-fortify")]
        let providers = providers.replace(
            openrust::foundation::ProviderId::of::<openrust::auth::fortify::FortifyServiceProvider>(
            ),
            Box::new(crate::app::providers::AuthenticationProfileProvider::default()),
        );
        providers
            .except([openrust::foundation::ProviderId::of::<
                openrust::translation::TranslationServiceProvider,
            >()])
            .merge([
                Box::new(crate::app::providers::translation_sources::TranslationSourcesProvider)
                    as Box<dyn ServiceProvider>,
            ])
    }

    fn package_providers(_config: &Self::Config) -> Vec<openrust::foundation::PackageProvider> {
        packages::providers()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        providers::application_providers(config)
    }

    fn configure_providers(
        config: &Self::Config,
        builder: &mut openrust::foundation::ProviderBuilder,
    ) {
        let root = config.application_root();
        builder
            .class_descriptors(crate::modules::example::class_descriptors())
            .module_manifest(crate::modules::manifest(&root, &config.modules));
    }
}

/// 装配应用 API 的统一前缀与无状态中间件组，再加载应用 Web 声明。
pub(crate) fn routes(routes: openrust::routing::Router, prefix: &str) -> openrust::routing::Router {
    let routes = routes.group(
        openrust::routing::RouteGroup::new()
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
