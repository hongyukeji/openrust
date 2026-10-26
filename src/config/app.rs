//! Application identity and runtime environment settings.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{ConfigError, ENV_PREFIX, ENV_VAR, validate_namespace};

/// Runtime environment, selected by `APP_ENV` by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// Local development: verbose logs, API docs enabled, `.env` honoured.
    #[default]
    Development,
    /// Automated tests: fake drivers by default.
    Test,
    /// Production: JSON logs, no debug surfaces.
    Production,
}

impl Environment {
    /// Reads the environment from `APP_ENV`, defaulting to [`Environment::Development`].
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with_namespace(ENV_PREFIX)
    }

    /// Reads the environment selected by the configured application namespace.
    ///
    /// `namespace` must be an uppercase identifier ending in `_`, such as `SHOP_`.
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

    /// Lowercase name used in file names and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }

    /// `true` only for [`Environment::Production`].
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

/// `[app]` section: identity of the application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Human-readable name used in logs and mail.
    pub name: String,
    /// Runtime environment. Always overwritten by the selected namespace after loading.
    pub env: Environment,
    /// Public base URL used to build absolute links.
    pub url: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: "OpenRust".to_owned(),
            env: Environment::Development,
            url: "http://localhost:3000".to_owned(),
        }
    }
}
