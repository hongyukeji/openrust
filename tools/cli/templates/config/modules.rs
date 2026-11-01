//! 已编译模块的激活器和独立持久状态路径；源码发现配置尚未进入本批。

use openrust::config::{
    ConfigContext, ConfigError, FileModuleActivatorConfig, ModuleActivatorsConfig, ModulesConfig,
};

/// 默认状态文件相对应用根解析；可选择独立相对路径或绝对路径，不自动迁移已有文件。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<ModulesConfig, ConfigError> {
    Ok(ModulesConfig {
        activator: ctx.env_or("MODULES_ACTIVATOR", "file"),
        activators: ModuleActivatorsConfig {
            file: FileModuleActivatorConfig {
                statuses_file: ctx
                    .env_or("MODULES_STATUSES_FILE", "modules_statuses.json")
                    .into(),
            },
        },
    })
}
