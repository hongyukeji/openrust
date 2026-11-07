//! 应用身份和运行环境设置。

use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{ConfigError, ENV_PREFIX, validate_namespace};

/// 应用环境的精确名称；自定义名称、大小写与空字符串均予以保留。
#[derive(Debug, Clone, Default)]
pub enum Environment {
    /// Laravel `local`。
    Local,
    /// 旧应用骨架显式使用的 `development`。
    Development,
    /// 精确的 `dev`。
    Dev,
    /// 精确的 `test`。
    Test,
    /// Laravel `testing`。
    Testing,
    /// Laravel `production`。
    #[default]
    Production,
    /// 精确的 `prod`。
    Prod,
    /// 其他环境名称。
    Custom(String),
}

impl PartialEq for Environment {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Environment {}

impl Hash for Environment {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Environment {
    /// 从 `APP_ENV` 读取运行环境；未设置时默认为 [`Environment::Production`]。
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with_namespace(ENV_PREFIX)
    }

    /// 读取由配置的应用命名空间选择的运行环境。
    ///
    /// `namespace` 必须是以 `_` 结尾的大写标识符，例如 `SHOP_`。
    pub fn from_env_with_namespace(namespace: &str) -> Result<Self, ConfigError> {
        validate_namespace(namespace)?;
        let key = format!("{namespace}ENV");
        Ok(std::env::var(&key).map_or_else(|_| Self::Production, Self::from))
    }

    /// 返回原始环境名称。
    pub fn as_str(&self) -> &str {
        match self {
            Self::Local => "local",
            Self::Development => "development",
            Self::Dev => "dev",
            Self::Test => "test",
            Self::Testing => "testing",
            Self::Production => "production",
            Self::Prod => "prod",
            Self::Custom(value) => value,
        }
    }

    /// 按 Laravel `Str::is` 的 `*` 通配规则匹配任一大小写敏感模式。
    pub fn matches<'a>(&self, patterns: impl IntoIterator<Item = &'a str>) -> bool {
        patterns
            .into_iter()
            .any(|pattern| wildcard_match(pattern, self.as_str()))
    }

    /// 仅当精确名称为 `local` 时返回 `true`。
    pub fn is_local(&self) -> bool {
        self.as_str() == "local"
    }

    /// 仅当精确名称为 `production` 时返回 `true`。
    pub fn is_production(&self) -> bool {
        self.as_str() == "production"
    }

    /// 保留框架原有生产安全限制；仅用于拒绝危险开发驱动与本地绕行。
    ///
    /// 与 Laravel 精确 `isProduction` 不同：历史输入中的 `prod`、大小写及首尾空白仍按生产保护。
    pub fn enforces_production_safety(&self) -> bool {
        matches!(
            self.as_str().trim().to_ascii_lowercase().as_str(),
            "prod" | "production"
        )
    }

    /// 仅当精确名称为 `testing` 时返回 `true`。
    pub fn running_unit_tests(&self) -> bool {
        self.as_str() == "testing"
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let Some((prefix, tail)) = pattern.split_once('*') else {
        return pattern == value;
    };
    let Some(mut remaining) = value.strip_prefix(prefix) else {
        return false;
    };
    let mut parts = tail.split('*').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() && !pattern.ends_with('*') {
            return remaining.ends_with(part);
        }
        let Some(position) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[position + part.len()..];
    }
    true
}

impl From<String> for Environment {
    fn from(value: String) -> Self {
        match value.as_str() {
            "local" => Self::Local,
            "development" => Self::Development,
            "dev" => Self::Dev,
            "test" => Self::Test,
            "testing" => Self::Testing,
            "production" => Self::Production,
            "prod" => Self::Prod,
            _ => Self::Custom(value),
        }
    }
}

impl Serialize for Environment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

impl FromStr for Environment {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(value.to_owned()))
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
    /// 运行环境；显式 `<PREFIX>ENV` 覆盖代码或文件配置，缺省为 `production`。
    pub env: Environment,
    /// 是否向开发者展示详细诊断；生产部署应明确关闭。
    pub debug: bool,
    /// 用于构造绝对链接的公开基础 URL。
    pub url: String,
    /// Laravel 应用加密使用的 cipher；默认 AES-256-CBC。
    pub cipher: String,
    /// 应用密钥的原始配置值；URL 签名不解码，应用加密在使用时解析。
    pub key: Option<String>,
    /// 用于验证旧 URL 签名和解密历史载荷的应用密钥。
    #[serde(deserialize_with = "previous_keys")]
    pub previous_keys: Vec<String>,
    /// 日期便捷读取的应用默认时区；不修改进程全局时区。
    pub timezone: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: "OpenRust".to_owned(),
            env: Environment::Production,
            debug: false,
            url: "http://localhost:3000".to_owned(),
            cipher: "AES-256-CBC".to_owned(),
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
            .field("debug", &self.debug)
            .field("url", &self.url)
            .field("cipher", &self.cipher)
            .field("key", &self.key.as_ref().map(|_| "[REDACTED]"))
            .field("previous_keys", &"[REDACTED]")
            .field("timezone", &self.timezone)
            .finish()
    }
}

pub(crate) fn supported_app_cipher_key_size(cipher: &str) -> Option<usize> {
    if cipher.eq_ignore_ascii_case("aes-128-cbc") || cipher.eq_ignore_ascii_case("aes-128-gcm") {
        Some(16)
    } else if cipher.eq_ignore_ascii_case("aes-256-cbc")
        || cipher.eq_ignore_ascii_case("aes-256-gcm")
    {
        Some(32)
    } else {
        None
    }
}

pub(crate) fn app_cipher_key_size(cipher: &str) -> usize {
    supported_app_cipher_key_size(cipher).unwrap_or(32)
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
