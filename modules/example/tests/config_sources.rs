//! 旧 Figment 应用测试沿生产代码配置加载器装配显式覆盖。

use openrust::config::{ConfigLoader, ConfigSources};
use openrust::core::Application;

pub(crate) fn application_sources(figment: &figment::Figment) -> openrust::Result<ConfigSources> {
    // 只适配已丢失声明序的旧 Figment；完整 ConfigSources 不经此入口降为快照。
    let entries = figment
        .clone()
        .extract::<serde_json::Map<String, serde_json::Value>>()
        .map_err(openrust::config::ConfigError::from)?;
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
