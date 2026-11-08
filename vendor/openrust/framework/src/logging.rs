//! 应用日志频道、记录和公开 driver 输出契约。

mod manager;
mod sinks;

pub use manager::{LogManager, LogServiceProvider, configuration};

use std::{fmt, sync::Arc};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{Result, support::BoxFuture};

/// 按 PSR 日志严重性从低到高排列的级别。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// 调试信息。
    #[default]
    Debug,
    /// 一般运行信息。
    Info,
    /// 正常但值得关注的情况。
    Notice,
    /// 警告。
    Warning,
    /// 运行错误。
    Error,
    /// 严重错误。
    Critical,
    /// 需要立即处置。
    Alert,
    /// 系统不可用。
    Emergency,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Notice => "NOTICE",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Critical => "CRITICAL",
            Self::Alert => "ALERT",
            Self::Emergency => "EMERGENCY",
        })
    }
}

/// 一次日志写入的不可变记录。栈内的 sink 接收同一频道名和记录。
pub struct LogRecord {
    at: DateTime<Utc>,
    channel: Arc<str>,
    level: LogLevel,
    message: String,
    context: Map<String, Value>,
}

impl LogRecord {
    /// 记录创建时间。
    pub fn at(&self) -> DateTime<Utc> {
        self.at
    }

    /// 写入该记录的 logger 名称。
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// 记录严重性。
    pub fn level(&self) -> LogLevel {
        self.level
    }

    /// 处理前的消息正文。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 本次写入附带的结构化字段。
    pub fn context(&self) -> &Map<String, Value> {
        &self.context
    }
}

/// 日志 driver 产出的异步写入目标。
pub trait LogSink: Send + Sync + 'static {
    /// 写入记录。后端错误应使用不包含记录或部署凭据的诊断信息。
    fn write<'a>(&'a self, record: &'a LogRecord) -> BoxFuture<'a, Result<()>>;
}

/// 可注入的日志频道句柄。克隆和重命名复用已解析的输出目标。
#[derive(Clone)]
pub struct Logger {
    name: Arc<str>,
    sink: Arc<dyn LogSink>,
    handlers: Option<Arc<[Arc<dyn LogSink>]>>,
}

impl Logger {
    /// 当前记录使用的 logger 名称。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 创建使用另一名称的 logger，保留输出目标、过滤和配置。
    #[must_use]
    pub fn with_name(&self, name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            sink: Arc::clone(&self.sink),
            handlers: self.handlers.clone(),
        }
    }

    /// 向当前频道写入一条消息。
    pub async fn log(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        context: Map<String, Value>,
    ) -> Result<()> {
        self.sink
            .write(&LogRecord {
                at: Utc::now(),
                channel: Arc::clone(&self.name),
                level,
                message: message.into(),
                context,
            })
            .await
    }
}
