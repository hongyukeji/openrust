//! 已编译业务模块的应用清单；声明代码与本次启动启用分别维护。

use openrust::core::ModuleManifest;

/// 登记已编译模块；状态文件按独立配置解析，资源路径仍使用同一应用根。
pub fn manifest(
    root: &std::path::Path,
    config: &openrust::config::ModulesConfig,
) -> ModuleManifest {
    ModuleManifest::from_config(root, config)
}
