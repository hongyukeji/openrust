//! 默认显式模块注册；文件发现及独立持久状态为可选模式。

use openrust::config::{ConfigContext, ConfigError};
use openrust_modules::config::{
    FileModuleActivatorConfig, ModuleActivatorsConfig, ModulePathsConfig, ModuleScanConfig,
    ModulesConfig,
};

/// 默认状态文件相对应用根解析；可选择独立相对路径或绝对路径，不自动迁移已有文件。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<ModulesConfig, ConfigError> {
    Ok(ModulesConfig {
        paths: ModulePathsConfig {
            modules: ctx.env_or("MODULES_PATH", "modules").into(),
        },
        scan: ModuleScanConfig::default(),
        activator: ctx.env_or("MODULES_ACTIVATOR", "registered"),
        activators: ModuleActivatorsConfig {
            file: FileModuleActivatorConfig {
                statuses_file: ctx
                    .env_or("MODULES_STATUSES_FILE", "modules_statuses.json")
                    .into(),
            },
        },
    })
}
