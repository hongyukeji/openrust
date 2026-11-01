//! 运行模块清单与显式编译绑定；元数据发现不执行未编译源码。

use openrust::core::ModuleManifest;

/// 从同一配置发现 module.json；Provider 与初始化文件须显式登记编译绑定。
pub fn manifest(
    root: &std::path::Path,
    config: &openrust::config::ModulesConfig,
) -> ModuleManifest {
    ModuleManifest::discover_from_config(root, config)
}
