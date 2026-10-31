//! 示例问候语只读取本模块声明的固定业务键。

use openrust::config::repository::ConfigRepository;
use openrust::{Error, Result};

/// 从当前已发布仓储读取公开的示例问候语。
pub(crate) fn greeting(repository: &ConfigRepository) -> Result<String> {
    Ok(repository.string_or("example.greeting", serde_json::json!("示例问候语尚未配置"))?)
}

/// 从应用的嵌套配置命名空间读取固定业务开关。
pub(crate) fn feature_enabled(repository: &ConfigRepository) -> Result<bool> {
    let value = repository.get_or("example.feature.enabled", serde_json::json!(false))?;
    value.as_bool().ok_or_else(|| {
        Error::custom(
            "example_feature_invalid",
            "example.feature.enabled 必须是布尔值",
        )
    })
}

/// 将业务复核后的固定问候语写入当前应用的配置仓储。
pub(crate) fn review(repository: &ConfigRepository) -> Result<String> {
    repository.set("example.greeting", serde_json::json!("示例问候语已复核"))?;
    greeting(repository)
}
