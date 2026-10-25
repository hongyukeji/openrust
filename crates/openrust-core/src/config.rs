//! Layered configuration.
//!
//! Sources are merged in this order, later layers overriding earlier ones:
//!
//! 1. crate defaults ([`FrameworkConfig::default`])
//! 2. `<config-dir>/default.toml`
//! 3. `<config-dir>/<env>.toml` where `<env>` comes from `OPENRUST_ENV` (`development` by default)
//! 4. environment variables prefixed `OPENRUST_`, nested with `__` (`OPENRUST_SERVER__PORT=4000` → `server.port`)
//!
//! Bootstrap reads only `OPENRUST_ENV` from `.env` to select the environment when the process
//! does not set it. Production skips loading the entire `.env` file.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Environment variable selecting the runtime environment.
pub const ENV_VAR: &str = "OPENRUST_ENV";
/// Environment variable selecting the configuration directory.
pub const CONFIG_DIR_VAR: &str = "OPENRUST_CONFIG_DIR";
/// Prefix of environment variables that override configuration keys.
pub const ENV_PREFIX: &str = "OPENRUST_";
/// Directory holding `default.toml` and per-environment files, relative to the working directory.
pub const DEFAULT_CONFIG_DIR: &str = "config";

/// Configuration failures. Only possible during bootstrap.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A value is missing, has the wrong type or a file could not be parsed.
    /// The message names the key and the source layer. Boxed to keep [`crate::Error`] small.
    #[error("{0}")]
    Source(Box<figment::Error>),
    /// `OPENRUST_ENV` holds a value other than `development`, `test` or `production`.
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

/// Runtime environment, selected by `OPENRUST_ENV`.
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
    /// Reads the environment from `OPENRUST_ENV`, defaulting to [`Environment::Development`].
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
    /// Runtime environment. Always overwritten by `OPENRUST_ENV` after loading.
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
    /// Response compression settings.
    pub compression: CompressionConfig,
    /// Cross-origin request settings.
    pub cors: CorsConfig,
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
            compression: CompressionConfig::default(),
            cors: CorsConfig::default(),
        }
    }
}

/// `[server.compression]` section: negotiated gzip and Brotli responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressionConfig {
    /// Enables response compression.
    pub enabled: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `[server.cors]` section: cross-origin response policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CorsConfig {
    /// Enables CORS responses.
    pub enabled: bool,
    /// Exact origins, or `["*"]` for every origin.
    pub allowed_origins: Vec<String>,
    /// Methods permitted in preflight requests.
    pub allowed_methods: Vec<String>,
    /// Request headers permitted in preflight requests.
    pub allowed_headers: Vec<String>,
    /// Response headers visible to browser clients.
    pub expose_headers: Vec<String>,
    /// Allows browser clients to send credentials.
    pub allow_credentials: bool,
    /// Preflight cache lifetime in seconds.
    pub max_age_secs: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_origins: Vec::new(),
            allowed_methods: ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]
                .map(str::to_owned)
                .to_vec(),
            allowed_headers: ["content-type", "authorization", "x-request-id"]
                .map(str::to_owned)
                .to_vec(),
            expose_headers: vec!["x-request-id".to_owned()],
            allow_credentials: false,
            max_age_secs: 600,
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

/// Reads only `OPENRUST_ENV` from `.env` without changing process variables.
pub(crate) fn dotenv_environment() -> Result<Environment, ConfigError> {
    let Ok(entries) = dotenvy::from_path_iter(".env") else {
        return Ok(Environment::Development);
    };
    for entry in entries.flatten() {
        if entry.0 == ENV_VAR && !entry.1.trim().is_empty() {
            return entry.1.parse();
        }
    }
    Ok(Environment::Development)
}

/// Builds the merged configuration source for one environment.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    config_dir: PathBuf,
    env: Environment,
}

impl ConfigLoader {
    /// Uses `OPENRUST_CONFIG_DIR` (or [`DEFAULT_CONFIG_DIR`]) and `OPENRUST_ENV`.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self::from_environment(Environment::from_env()?))
    }

    /// Uses `OPENRUST_CONFIG_DIR` (or [`DEFAULT_CONFIG_DIR`]) and the selected environment.
    pub(crate) fn from_environment(env: Environment) -> Self {
        Self::new(
            std::env::var_os(CONFIG_DIR_VAR)
                .filter(|value| !value.is_empty())
                .map_or_else(|| PathBuf::from(DEFAULT_CONFIG_DIR), PathBuf::from),
            env,
        )
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
            .merge(Toml::file(self.config_dir.join("default.toml")))
            .merge(Toml::file(
                self.config_dir.join(format!("{}.toml", self.env)),
            ))
            .merge(
                Env::raw()
                    .only(&["DATABASE_URL", "PORT"])
                    .map(|key| {
                        if key == "PORT" {
                            "server.port".into()
                        } else {
                            "database.url".into()
                        }
                    })
                    .split("."),
            )
            .merge(
                Env::prefixed(ENV_PREFIX)
                    .filter(|key| key != "ENV")
                    .split("__"),
            )
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
                "config/default.toml",
                "[server]\nport = 4000\nhost = \"0.0.0.0\"",
            )?;
            jail.create_file("config/test.toml", "[server]\nport = 5000")?;
            jail.set_env("OPENRUST_SERVER__HOST", "10.0.0.1");

            let loader = ConfigLoader::new("config", Environment::Test);
            let config = loader
                .load_framework()
                .map_err(|e| figment::Error::from(e.to_string()))?;
            assert_eq!(
                config.server.port, 5000,
                "environment file overrides default.toml"
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
    fn config_dir_environment_override_loads_selected_directory() {
        figment::Jail::expect_with(|jail| {
            jail.set_env(CONFIG_DIR_VAR, "");
            jail.clear_env();
            jail.create_dir("custom")?;
            jail.create_file("custom/default.toml", "[server]\nport = 4321")?;
            jail.set_env(CONFIG_DIR_VAR, "custom");
            let loader = ConfigLoader::from_env()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.config_dir(), Path::new("custom"));
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4321
            );
            Ok(())
        });
    }

    #[test]
    fn config_dir_defaults_to_working_directory_config() {
        figment::Jail::expect_with(|jail| {
            jail.set_env(CONFIG_DIR_VAR, "");
            jail.clear_env();
            jail.create_dir("config")?;
            jail.create_file("config/default.toml", "[server]\nport = 4567")?;
            let loader = ConfigLoader::from_env()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.config_dir(), Path::new("config"));
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4567
            );
            Ok(())
        });
    }

    #[test]
    fn wrong_type_reports_key_name() {
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file("config/default.toml", "[server]\nport = \"not-a-port\"")?;
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

    #[test]
    fn server_middleware_defaults_are_complete() {
        let server = ServerConfig::default();
        assert!(server.compression.enabled);
        assert!(!server.cors.enabled);
        assert_eq!(server.cors.allowed_origins, Vec::<String>::new());
        assert_eq!(server.cors.allowed_methods.len(), 6);
        assert_eq!(
            server.cors.allowed_headers,
            ["content-type", "authorization", "x-request-id"]
        );
        assert_eq!(server.cors.expose_headers, ["x-request-id"]);
        assert!(!server.cors.allow_credentials);
        assert_eq!(server.cors.max_age_secs, 600);
    }

    #[test]
    fn partial_cors_toml_keeps_field_defaults() -> Result<(), toml::de::Error> {
        let parsed: FrameworkConfig = toml::from_str(
            "[server.cors]\nenabled = true\nallowed_origins = [\"https://example.com\"]",
        )?;
        assert!(parsed.server.cors.enabled);
        assert_eq!(parsed.server.cors.allowed_origins, ["https://example.com"]);
        assert_eq!(
            parsed.server.cors.allowed_methods,
            CorsConfig::default().allowed_methods
        );
        assert_eq!(parsed.server.cors.max_age_secs, 600);
        Ok(())
    }

    #[test]
    fn cors_list_environment_override_uses_figment_array_syntax() {
        figment::Jail::expect_with(|jail| {
            jail.set_env(
                "OPENRUST_SERVER__CORS__ALLOWED_ORIGINS",
                "[\"https://example.com\", \"https://other.example\"]",
            );
            let config = ConfigLoader::new("missing-config", Environment::Test)
                .load_framework()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(
                config.server.cors.allowed_origins,
                ["https://example.com", "https://other.example"]
            );
            Ok(())
        });
    }

    #[test]
    fn conventional_variables_map_to_sections_and_prefixed_values_win() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("DATABASE_URL", "postgres://common/database");
            jail.set_env("PORT", "4100");
            let loader = ConfigLoader::new("missing-config", Environment::Test);
            let database: serde_json::Value = loader.figment().extract_inner("database")?;
            assert_eq!(database["url"], "postgres://common/database");
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4100
            );

            jail.set_env("OPENRUST_DATABASE__URL", "postgres://prefixed/database");
            jail.set_env("OPENRUST_SERVER__PORT", "4200");
            let database: serde_json::Value = loader.figment().extract_inner("database")?;
            assert_eq!(database["url"], "postgres://prefixed/database");
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4200
            );
            Ok(())
        });
    }

    #[test]
    fn environment_selection_uses_only_openrust_env() {
        figment::Jail::expect_with(|jail| {
            jail.set_env(concat!("APP", "_ENV"), "production");
            assert_eq!(
                Environment::from_env().map_err(|error| figment::Error::from(error.to_string()))?,
                Environment::Development
            );
            jail.set_env("OPENRUST_ENV", "test");
            assert_eq!(
                Environment::from_env().map_err(|error| figment::Error::from(error.to_string()))?,
                Environment::Test
            );
            Ok(())
        });
    }
}
