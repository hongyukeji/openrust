//! 应用启动时加载的 Provider 清单。

use openrust::support::ServiceProvider;

pub(super) fn application_providers(
    config: &super::Configuration,
) -> Vec<Box<dyn ServiceProvider>> {
    // 迁移注册不连接数据库；执行命令时再沿统一 Db 验证实际连接。
    let providers: Vec<Box<dyn ServiceProvider>> = vec![
        Box::new(crate::app::providers::AppServiceProvider),
        Box::new(openrust::database::MigrationServiceProvider::<
            crate::database::Migrator,
        >::new()),
    ];
    #[cfg(feature = "passport")]
    let providers = {
        let mut providers = providers;
        providers.push(Box::new(openrust_passport::PassportServiceProvider::new(
            config.application_root().join("storage"),
        )));
        providers
    };
    #[cfg(not(feature = "passport"))]
    let _ = config;
    providers
}
