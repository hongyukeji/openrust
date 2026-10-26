//! 日志配置。

use serde::{Deserialize, Serialize};

/// `[log]` 区段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// `tracing` filter 指令，例如 `info` 或 `info,openrust=debug`。`RUST_LOG` 优先级更高。
    pub level: String,
    /// 输出格式。
    pub format: LogFormat,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_owned(),
            format: LogFormat::Text,
        }
    }
}

/// 日志输出格式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// 供终端阅读的文本行。
    #[default]
    Text,
    /// 每行一个 JSON 对象，供日志采集器使用。
    Json,
}
