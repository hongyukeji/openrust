//! Session 默认值。

use openrust::config::{ConfigContext, ConfigError, SessionConfig};

/// 使用框架公开的 Session 默认设置。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<SessionConfig, ConfigError> {
    Ok(SessionConfig::default())
}
