//! 日志输出的应用默认值。

use openrust::config::{ConfigContext, ConfigError, LogConfig, LogFormat};

/// `RUST_LOG` 仍可在统一日志初始化阶段覆盖 filter。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<LogConfig, ConfigError> {
    Ok(LogConfig {
        level: "info".to_owned(),
        format: LogFormat::Text,
    })
}
