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

/// 读取构造弱标量的应用弃用策略，默认通道不写日志。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn diagnostics(ctx: &ConfigContext) -> Result<serde_json::Value, ConfigError> {
    Ok(serde_json::json!({
        "deprecations": {
            "channel": ctx.nullable_string_or("LOG_DEPRECATIONS_CHANNEL", Some("null")),
            "trace": ctx.nullable_bool_or("LOG_DEPRECATIONS_TRACE", Some(false)),
            "while_testing": ctx.nullable_bool_or("LOG_DEPRECATIONS_WHILE_TESTING", Some(false)),
        }
    }))
}
