//! 旧 Figment 应用测试沿生产代码配置加载器装配显式覆盖。

use openrust::config::{ConfigLoader, ConfigSources};
use openrust::core::Application;

pub(crate) fn application_sources(figment: &figment::Figment) -> openrust::Result<ConfigSources> {
    // 只适配已丢失声明序的旧 Figment；完整 ConfigSources 不经此入口降为快照。
    let mut entries = figment
        .clone()
        .extract::<serde_json::Map<String, serde_json::Value>>()
        .map_err(openrust::config::ConfigError::from)?;
    // FrameworkConfig 不再拥有模块配置；旧完整快照显式补回扩展的 file 默认，
    // 避免经应用代码默认 registered 改变已有发现/持久状态 fixture 的选择。
    entries.entry("modules".to_owned()).or_insert(
        serde_json::to_value(openrust_modules::ModulesConfig::default())
            .map_err(openrust::Error::internal)?,
    );
    // 旧 fixture 明确提供的完整快照在函数求值前覆盖；不代表应用默认继承测试。
    ConfigLoader::application_code_sources_with_overrides(
        env!("CARGO_MANIFEST_DIR"),
        "APP_",
        None,
        &crate::bootstrap::Server::config(),
        entries,
    )
    .map_err(Into::into)
}

#[test]
fn legacy_snapshot_keeps_extension_file_defaults_and_preserves_explicit_module_selection()
-> openrust::Result<()> {
    for explicit in [None, Some("registered"), Some("file-live")] {
        let mut source = serde_json::json!({});
        if let Some(activator) = explicit {
            source["modules"] = serde_json::json!({
                "activator":activator,
                "paths":{"modules":"custom/business"},
                "scan":{"enabled":true,"paths":["custom/*"]},
                "activators":{"file":{"statuses-file":"state/selected.json"}}
            });
        }
        let figment = figment::Figment::from(figment::providers::Serialized::defaults(source));
        let sources = application_sources(&figment)?;
        let module = sources
            .figment()
            .extract_inner::<openrust_modules::ModulesConfig>("modules")
            .map_err(openrust::config::ConfigError::from)?;
        assert_eq!(module.activator, explicit.unwrap_or("file"));
        if explicit.is_some() {
            assert_eq!(
                module.paths.modules,
                std::path::PathBuf::from("custom/business")
            );
            assert_eq!(
                module.activators.file.statuses_file,
                std::path::PathBuf::from("state/selected.json")
            );
        } else {
            assert_eq!(module, openrust_modules::ModulesConfig::default());
        }
    }
    Ok(())
}
