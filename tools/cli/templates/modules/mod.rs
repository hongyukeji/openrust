//! 已编译业务模块的应用清单；声明代码与本次启动启用分别维护。

use openrust::core::ModuleManifest;

/// 登记已编译模块；从运行工作目录读取 `modules_statuses.json`。
pub fn manifest(root: &std::path::Path) -> ModuleManifest {
    ModuleManifest::from_file(root.join("modules_statuses.json"))
}
