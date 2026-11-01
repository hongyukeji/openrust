//! API 响应的应用默认值。

use openrust::config::{ApiConfig, ConfigContext, ConfigError, HttpStatusMode};

/// 保留语义 HTTP 状态；仅显式选择 `AlwaysOk` 时统一返回 HTTP 200。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<ApiConfig, ConfigError> {
    Ok(ApiConfig {
        http_status: HttpStatusMode::Semantic,
    })
}
