//! 编译可用模块与唯一默认注册清单；Rust mod 本身不表示启用。

pub(crate) mod example;

/// 默认显式注册即启用；文件发现和持久状态须由应用显式选择。
pub(crate) fn manifest(
    root: &std::path::Path,
    config: &openrust::config::ModulesConfig,
) -> openrust::core::ModuleManifest {
    if config.activator == "registered" {
        openrust::core::ModuleManifest::new()
            .with_application_root(root)
            .module(example::registered_descriptor(root, config))
            .enable("example")
    } else {
        example::manifest(root, config)
    }
}
