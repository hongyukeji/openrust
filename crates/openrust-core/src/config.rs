//! Layered configuration.
//!
//! Sources are merged in this order, later layers overriding earlier ones:
//!
//! 1. crate defaults ([`FrameworkConfig::default`])
//! 2. `config/app.toml`
//! 3. `config/<env>.toml` where `<env>` comes from `APP_ENV` (`development` by default)
//! 4. environment variables prefixed `APP__`, nested with `__` (`APP__SERVER__PORT=4000` → `server.port`)
//!
//! `.env` files are loaded by [`load_dotenv`] before anything else so they can supply `APP_ENV`
//! and secrets during development. Production deployments should not ship a `.env` file.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Environment variable selecting the runtime environment.
pub const ENV_VAR: &str = "APP_ENV";
/// Prefix of environment variables that override configuration keys.
pub const ENV_PREFIX: &str = "APP__";
/// Directory holding `app.toml` and per-environment files, relative to the working directory.
pub const DEFAULT_CONFIG_DIR: &str = "config";

/// Configuration failures. Only possible during bootstrap.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A value is missing, has the wrong type or a file could not be parsed.
    /// The message names the key and the source layer. Boxed to keep [`crate::Error`] small.
    #[error("{0}")]
    Source(Box<figment::Error>),
    /// `APP_ENV` holds a value other than `development`, `test` or `production`.
    #[error("unknown environment `{0}` in {ENV_VAR}; expected development, test or production")]
    UnknownEnvironment(String),
    /// Values were loaded but violate a framework invariant.
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

impl From<figment::Error> for ConfigError {
    fn from(source: figment::Error) -> Self {
        Self::Source(Box::new(source))
    }
}

/// Runtime environment, selected by `APP_ENV`.
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
        match std::env::var(ENV_VAR) {
            Ok(value) if !value.trim().is_empty() => value.parse(),
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
            other => Err(ConfigError::UnknownEnvironment(other.to_owned())),
        }
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Configuration owned by the framework. Applications add their own sections alongside.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameworkConfig {
    /// `[app]` section.
    pub app: AppConfig,
    /// `[server]` section.
    pub server: ServerConfig,
    /// `[log]` section.
    pub log: LogConfig,
}

/// `[app]` section: identity of the application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Human-readable name used in logs and mail.
    pub name: String,
    /// Runtime environment. Always overwritten by `APP_ENV` after loading.
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

/// `[server]` section: HTTP listener settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Interface to bind.
    pub host: String,
    /// TCP port to bind.
    pub port: u16,
    /// Per-request timeout in seconds; exceeding it yields `408 Request Timeout`.
    pub request_timeout_secs: u64,
    /// Grace period for in-flight requests after a shutdown signal.
    pub shutdown_timeout_secs: u64,
}

impl ServerConfig {
    /// `host:port` suitable for binding a listener.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: 3000,
            request_timeout_secs: 30,
            shutdown_timeout_secs: 10,
        }
    }
}

/// `[log]` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// `tracing` filter directive, e.g. `info` or `info,openrust=debug`. `RUST_LOG` takes precedence.
    pub level: String,
    /// Output format.
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

/// Log output format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable lines for terminals.
    #[default]
    Text,
    /// One JSON object per line for log collectors.
    Json,
}

/// Marker for applications without configuration of their own: `type Config = NoConfig;`.
///
/// The unit type `()` cannot be used because serde refuses to read a map into a unit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoConfig {}

/// Loads a `.env` file from the working directory if present and returns its path.
///
/// Existing environment variables are never overwritten.
pub fn load_dotenv() -> Option<PathBuf> {
    dotenvy::dotenv().ok()
}

/// Builds the merged configuration source for one environment.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    config_dir: PathBuf,
    env: Environment,
}

impl ConfigLoader {
    /// Uses [`DEFAULT_CONFIG_DIR`] and the environment from `APP_ENV`.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self::new(DEFAULT_CONFIG_DIR, Environment::from_env()?))
    }

    /// Uses an explicit directory and environment.
    pub fn new(config_dir: impl Into<PathBuf>, env: Environment) -> Self {
        Self {
            config_dir: config_dir.into(),
            env,
        }
    }

    /// Selected environment.
    pub fn environment(&self) -> Environment {
        self.env
    }

    /// Directory the TOML files are read from.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// The layered [`Figment`] every section is extracted from.
    pub fn figment(&self) -> Figment {
        Figment::from(Serialized::defaults(FrameworkConfig::default()))
            .merge(Toml::file(self.config_dir.join("app.toml")))
            .merge(Toml::file(
                self.config_dir.join(format!("{}.toml", self.env)),
            ))
            .merge(Env::prefixed(ENV_PREFIX).split("__"))
    }

    /// Extracts the whole merged configuration into `T`.
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, ConfigError> {
        Ok(self.figment().extract()?)
    }

    /// Extracts the framework sections and pins `app.env` to the selected environment.
    pub fn load_framework(&self) -> Result<FrameworkConfig, ConfigError> {
        let mut config: FrameworkConfig = self.load()?;
        config.app.env = self.env;
        Ok(config)
    }
}

/// Replaces values whose key looks secret (`password`, `secret`, `token`, `key`, `dsn`) with `********`.
pub fn redact_secrets(value: &mut serde_json::Value) {
    const SECRET_MARKERS: [&str; 5] = ["password", "secret", "token", "key", "dsn"];
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let lowered = key.to_ascii_lowercase();
                if SECRET_MARKERS.iter().any(|m| lowered.contains(m)) && !child.is_object() {
                    *child = serde_json::Value::String("********".to_owned());
                } else {
                    redact_secrets(child);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

#[cfg(test)]
// `figment::Jail` closures must return `figment::Result`, whose error type is large by design.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;

    #[test]
    fn layers_override_in_order() {
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file(
                "config/app.toml",
                "[server]\nport = 4000\nhost = \"0.0.0.0\"",
            )?;
            jail.create_file("config/test.toml", "[server]\nport = 5000")?;
            jail.set_env("APP__SERVER__HOST", "10.0.0.1");

            let loader = ConfigLoader::new("config", Environment::Test);
            let config = loader
                .load_framework()
                .map_err(|e| figment::Error::from(e.to_string()))?;
            assert_eq!(
                config.server.port, 5000,
                "environment file overrides app.toml"
            );
            assert_eq!(config.server.host, "10.0.0.1", "env var overrides files");
            assert_eq!(
                config.app.env,
                Environment::Test,
                "env is pinned by the loader"
            );
            assert_eq!(config.app.name, "OpenRust", "defaults fill missing keys");
            Ok(())
        });
    }

    #[test]
    fn wrong_type_reports_key_name() {
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file("config/app.toml", "[server]\nport = \"not-a-port\"")?;
            let error = ConfigLoader::new("config", Environment::Development)
                .load_framework()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains("server.port"), "message was: {error}");
            Ok(())
        });
    }

    #[test]
    fn unknown_environment_is_rejected() {
        assert!(matches!(
            "staging".parse::<Environment>(),
            Err(ConfigError::UnknownEnvironment(value)) if value == "staging"
        ));
        assert_eq!(
            "PROD".parse::<Environment>().ok(),
            Some(Environment::Production)
        );
    }

    #[test]
    fn secrets_are_redacted_recursively() {
        let mut value = serde_json::json!({
            "database": { "url": "postgres://u:p@h/db", "password": "hunter2" },
            "mail": { "smtp": { "token": "abc" } },
            "app": { "name": "Blog" }
        });
        redact_secrets(&mut value);
        assert_eq!(value["database"]["password"], "********");
        assert_eq!(value["mail"]["smtp"]["token"], "********");
        assert_eq!(value["app"]["name"], "Blog");
    }
}
