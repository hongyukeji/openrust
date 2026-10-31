//! 示例问候语只读取本模块声明的固定业务键。

use openrust::config::repository::ConfigRepository;
use openrust::{Error, Result};

/// 从当前已发布仓储读取公开的示例问候语。
pub(crate) fn greeting(repository: &ConfigRepository) -> Result<String> {
    let value = repository.get_or("example.greeting", serde_json::json!("示例问候语尚未配置"))?;
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::custom("example_greeting_invalid", "example.greeting 必须是字符串"))
}

/// 将业务复核后的固定问候语写入当前应用的配置仓储。
pub(crate) fn review(repository: &ConfigRepository) -> Result<String> {
    repository.set("example.greeting", serde_json::json!("示例问候语已复核"))?;
    greeting(repository)
}
