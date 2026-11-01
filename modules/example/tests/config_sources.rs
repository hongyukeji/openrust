//! 旧 Figment 应用测试沿生产代码配置加载器装配显式覆盖。

use openrust::config::{ConfigLoader, ConfigSources};
use openrust::core::Application;

pub(crate) fn application_sources(figment: &figment::Figment) -> openrust::Result<ConfigSources> {
    // 只适配已丢失声明序的旧 Figment；完整 ConfigSources 不经此入口降为快照。
    let entries = figment
        .clone()
        .extract::<serde_json::Map<String, serde_json::Value>>()
        .map_err(openrust::config::ConfigError::from)?;
    let base = ConfigLoader::application_code_sources(
        env!("CARGO_MANIFEST_DIR"),
        "APP_",
        None,
        &crate::bootstrap::Server::config(),
    )?;
    // 原 fixture 的完整最终快照优先，null/disabled/memory 保持其服务隔离策略。
    base.merge_ordered_entries(entries).map_err(Into::into)
}
