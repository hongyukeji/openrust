//! 应用 Provider 清单，对应 Laravel 的 `bootstrap/providers.php`。

use openrust::core::ServiceProvider;

pub(super) fn application_providers(
    config: &super::Configuration,
) -> Vec<Box<dyn ServiceProvider>> {
    #[allow(unused_mut, reason = "数据库 Provider 仅在对应 feature 启用时追加")]
    let mut providers: Vec<Box<dyn ServiceProvider>> =
        vec![Box::new(crate::app::providers::AppServiceProvider)];
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    if config.uses_database() {
        providers.push(Box::new(openrust::db::MigrationServiceProvider::<
            openrust::db::FrameworkMigrator,
        >::new()));
    }
    #[cfg(not(any(feature = "db-postgres", feature = "db-mysql")))]
    let _ = config;
    providers
}
