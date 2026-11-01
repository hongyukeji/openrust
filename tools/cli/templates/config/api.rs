//! API 响应默认值。

use openrust::config::{ApiConfig, ConfigContext, ConfigError};

/// 使用语义 HTTP 状态。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<ApiConfig, ConfigError> {
    Ok(ApiConfig::default())
}
