//! 应用身份和运行环境设置。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{ConfigError, ENV_PREFIX, ENV_VAR, validate_namespace};

/// 运行环境，默认由 `APP_ENV` 选择。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// 本地开发：详细日志、启用 API 文档并读取 `.env`。
    #[default]
    Development,
    /// 自动化测试：默认使用 fake driver。
    Test,
    /// 生产环境：JSON 日志，不启用调试入口。
    Production,
}

impl Environment {
    /// 从 `APP_ENV` 读取运行环境；未设置时默认为 [`Environment::Development`]。
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with_namespace(ENV_PREFIX)
    }

    /// 读取由配置的应用命名空间选择的运行环境。
    ///
    /// `namespace` 必须是以 `_` 结尾的大写标识符，例如 `SHOP_`。
    pub fn from_env_with_namespace(namespace: &str) -> Result<Self, ConfigError> {
        validate_namespace(namespace)?;
        let key = format!("{namespace}ENV");
        match std::env::var(&key) {
            Ok(value) if !value.trim().is_empty() => {
                value.parse().map_err(|error: ConfigError| match error {
                    ConfigError::UnknownEnvironment { value, .. } => {
                        ConfigError::UnknownEnvironment { key, value }
                    }
                    other => other,
                })
            }
            _ => Ok(Self::Development),
        }
    }

    /// 用于文件名和日志的小写名称。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }

    /// 仅当环境为 [`Environment::Production`] 时返回 `true`。
    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

impl FromStr for Environment {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "development" | "dev" | "local" => Ok(Self::Development),
            "test" | "testing" => Ok(Self::Test),
            "production" | "prod" => Ok(Self::Production),
            other => Err(ConfigError::UnknownEnvironment {
                key: ENV_VAR.to_owned(),
                value: other.to_owned(),
            }),
        }
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `[app]` 区段：应用身份信息。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 用于日志和邮件的可读名称。
    pub name: String,
    /// 运行环境。加载配置后始终会被所选命名空间的值覆盖。
    pub env: Environment,
    /// 用于构造绝对链接的公开基础 URL。
    pub url: String,
    /// URL 签名使用的原始应用密钥；不进行 Base64 解码。
    pub key: Option<String>,
    /// 仅用于验证旧 URL 签名的密钥。
    #[serde(deserialize_with = "previous_keys")]
    pub previous_keys: Vec<String>,
    /// 日期便捷读取的应用默认时区；不修改进程全局时区。
    pub timezone: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: "OpenRust".to_owned(),
            env: Environment::Development,
            url: "http://localhost:3000".to_owned(),
            key: None,
            previous_keys: Vec::new(),
            timezone: "UTC".to_owned(),
        }
    }
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("name", &self.name)
            .field("env", &self.env)
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_| "[REDACTED]"))
            .field("previous_keys", &"[REDACTED]")
            .field("timezone", &self.timezone)
            .finish()
    }
}

fn previous_keys<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(|value| match value {
                serde_json::Value::String(value) => Ok(value),
                serde_json::Value::Number(value) => Ok(value.to_string()),
                _ => Err(serde::de::Error::custom(
                    "app.previous_keys must contain strings or numbers",
                )),
            })
            .collect(),
        serde_json::Value::String(value) => Ok(value
            .split(',')
            .filter(|key| !key.is_empty() && *key != "0")
            .map(str::to_owned)
            .collect()),
        serde_json::Value::Null => Ok(Vec::new()),
        _ => Err(serde::de::Error::custom(
            "app.previous_keys must be an array or comma-separated string",
        )),
    }
}

impl AppConfig {
    pub(crate) fn resolved_timezone(
        &self,
    ) -> Result<crate::support::ResolvedTimezone, ConfigError> {
        crate::support::ResolvedTimezone::parse(&self.timezone).map_err(|_| {
            ConfigError::Invalid("app.timezone: 时区名称无效（源值未公开）".to_owned())
        })
    }
}
