//! 应用启动时加载的 Provider 清单。

use openrust::support::ServiceProvider;

pub(super) fn application_providers(
    _config: &super::Configuration,
) -> Vec<Box<dyn ServiceProvider>> {
    // 迁移注册不连接数据库；执行命令时再沿统一 Db 验证实际连接。
    vec![
        Box::new(crate::app::providers::AppServiceProvider),
        Box::new(openrust::database::MigrationServiceProvider::<
            crate::database::Migrator,
        >::new()),
    ]
}
