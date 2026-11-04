//! 密码验证工作流的可编辑哈希策略；算法驱动仍由已实现密码服务提供。

use openrust::config::{ConfigContext, ConfigError};
use serde::Serialize;

/// 已实现的登录重哈希策略。
#[derive(Serialize)]
pub struct Hashing {
    rehash_on_login: bool,
}

/// 保留官方登录时按需重哈希默认，不因随机盐不同重算。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<Hashing, ConfigError> {
    Ok(Hashing {
        rehash_on_login: true,
    })
}
