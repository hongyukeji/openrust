//! 运行模块清单与显式编译绑定；元数据发现不执行未编译源码。

use openrust_modules::ModuleManifest;

/// 默认只加载显式登记模块；其他激活器按配置发现 module.json 与文件状态。
pub(crate) fn manifest(
    root: &std::path::Path,
    config: &openrust_modules::config::ModulesConfig,
) -> ModuleManifest {
    if config.activator == "registered" {
        ModuleManifest::new().with_application_root(root)
    } else {
        ModuleManifest::discover_from_config(root, config)
    }
}
