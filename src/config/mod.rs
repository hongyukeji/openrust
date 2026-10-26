//! Layered configuration.
//!
//! Configuration definitions live in the domain modules under `config` and are merged by one
//! [`ConfigLoader`]. Runtime TOML files stay in the application root's `config/` directory.
//!
//! Sources are merged in this order, later layers overriding earlier ones:
//!
//! 1. crate defaults ([`FrameworkConfig::default`])
//! 2. `<config-dir>/default.toml`
//! 3. `<config-dir>/<env>.toml` where `<env>` comes from `APP_ENV` (`development` by default)
//! 4. variables loaded from local `.env` in non-production, then process variables, prefixed with
//!    `APP_` and nested with `__` (`APP_SERVER__PORT=4000` → `server.port`)
//!
//! Bootstrap reads only `APP_ENV` from `.env` to select the environment when the process does not
//! set it. Production skips loading the entire `.env` file. Existing process variables take
//! precedence over `.env` values.

use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub mod api;
pub mod app;
pub mod auth;
pub mod database;
pub mod logger;
pub mod mailer;
pub mod server;
pub mod sms;

pub use api::{ApiConfig, HttpStatusMode};
pub use app::{AppConfig, Environment};
pub use auth::{JwtConfig, PasswordConfig, SessionConfig};
pub use database::DatabaseConfig;
pub use logger::{LogConfig, LogFormat};
pub use mailer::MailConfig;
pub use server::{CompressionConfig, CorsConfig, ServerConfig};
pub use sms::SmsConfig;

/// Environment variable selecting the runtime environment.
pub const ENV_VAR: &str = "APP_ENV";
/// Environment variable selecting the configuration directory.
pub const CONFIG_DIR_VAR: &str = "APP_CONFIG_DIR";
/// Prefix of environment variables that override configuration keys.
pub const ENV_PREFIX: &str = "APP_";
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
    /// The selected namespace's environment key holds an unsupported value.
    #[error("unknown environment `{value}` in {key}; expected development, test or production")]
    UnknownEnvironment {
        /// Environment variable key that supplied the invalid value.
        key: String,
        /// Unrecognized environment value.
        value: String,
    },
    /// Values were loaded but violate a framework invariant.
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

impl From<figment::Error> for ConfigError {
    fn from(source: figment::Error) -> Self {
        Self::Source(Box::new(source))
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
    /// `[api]` section.
    pub api: ApiConfig,
    /// `[log]` section.
    pub log: LogConfig,
    /// `[i18n]` locale defaults used by `I18nPlugin`.
    pub i18n: crate::i18n::I18nConfig,
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

/// Reads only the selected namespace's environment key from `.env` without changing process variables.
pub(crate) fn dotenv_environment(namespace: &str) -> Result<Environment, ConfigError> {
    validate_namespace(namespace)?;
    let key = format!("{namespace}ENV");
    let Ok(entries) = dotenvy::from_path_iter(".env") else {
        return Ok(Environment::Development);
    };
    for entry in entries.flatten() {
        if entry.0 == key && !entry.1.trim().is_empty() {
            return entry.1.parse().map_err(|error: ConfigError| match error {
                ConfigError::UnknownEnvironment { value, .. } => {
                    ConfigError::UnknownEnvironment { key, value }
                }
                other => other,
            });
        }
    }
    Ok(Environment::Development)
}

/// Builds the merged configuration source for one environment.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    config_dir: PathBuf,
    env: Environment,
    namespace: String,
}

impl ConfigLoader {
    /// Uses `APP_CONFIG_DIR` (or [`DEFAULT_CONFIG_DIR`]) and `APP_ENV`.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with_namespace(ENV_PREFIX)
    }

    /// Uses the selected namespace's `<PREFIX>ENV` and `<PREFIX>CONFIG_DIR` values.
    pub fn from_env_with_namespace(namespace: &str) -> Result<Self, ConfigError> {
        let env = Environment::from_env_with_namespace(namespace)?;
        Self::from_environment_with_namespace(env, namespace)
    }

    /// Uses the selected namespace's `<PREFIX>CONFIG_DIR` (or [`DEFAULT_CONFIG_DIR`]).
    pub(crate) fn from_environment_with_namespace(
        env: Environment,
        namespace: &str,
    ) -> Result<Self, ConfigError> {
        validate_namespace(namespace)?;
        let config_dir_key = format!("{namespace}CONFIG_DIR");
        let config_dir = std::env::var_os(&config_dir_key)
            .filter(|value| !value.is_empty())
            .map_or_else(|| PathBuf::from(DEFAULT_CONFIG_DIR), PathBuf::from);
        Self::new(DEFAULT_CONFIG_DIR, env).with_config_dir_and_namespace(config_dir, namespace)
    }

    /// Uses an explicit directory and environment.
    pub fn new(config_dir: impl Into<PathBuf>, env: Environment) -> Self {
        Self {
            config_dir: config_dir.into(),
            env,
            namespace: ENV_PREFIX.to_owned(),
        }
    }

    fn with_config_dir_and_namespace(
        mut self,
        config_dir: PathBuf,
        namespace: &str,
    ) -> Result<Self, ConfigError> {
        validate_namespace(namespace)?;
        self.config_dir = config_dir;
        namespace.clone_into(&mut self.namespace);
        Ok(self)
    }

    /// Returns a loader that reads only the selected application namespace.
    ///
    /// The namespace must be an uppercase identifier ending in `_`, such as `SHOP_`.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Result<Self, ConfigError> {
        let namespace = namespace.into();
        validate_namespace(&namespace)?;
        self.namespace = namespace;
        Ok(self)
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
                Env::prefixed(&self.namespace)
                    .filter(|key| key != "ENV" && key != "CONFIG_DIR")
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

fn validate_namespace(namespace: &str) -> Result<(), ConfigError> {
    let valid = namespace.ends_with('_')
        && namespace.len() > 1
        && namespace.strip_suffix('_').is_some_and(|prefix| {
            prefix
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_uppercase)
                && prefix
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(ConfigError::Invalid(
            "configuration namespace must be an uppercase identifier ending in `_` (for example `APP_`)".into(),
        ))
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
    fn domain_modules_preserve_capability_config_paths() {
        #[cfg(feature = "auth")]
        {
            let _: Option<crate::auth::JwtConfig> = None::<auth::JwtConfig>;
            let _: Option<crate::auth::PasswordConfig> = None::<auth::PasswordConfig>;
            let _: Option<crate::auth::SessionConfig> = None::<auth::SessionConfig>;
        }
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        {
            let _: Option<crate::db::DatabaseConfig> = None::<database::DatabaseConfig>;
        }
        let _: Option<crate::mail::MailConfig> = None::<mailer::MailConfig>;
    }

    #[test]
    fn layers_override_in_order() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file(
                "config/default.toml",
                "[server]\nport = 4000\nhost = \"0.0.0.0\"",
            )?;
            jail.create_file("config/test.toml", "[server]\nport = 5000")?;
            jail.set_env("APP_SERVER__HOST", "10.0.0.1");

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
        let _environment_guard = crate::lock_test_env();
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
        let _environment_guard = crate::lock_test_env();
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
        let _environment_guard = crate::lock_test_env();
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
            Err(ConfigError::UnknownEnvironment { value, .. }) if value == "staging"
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
    fn api_http_status_defaults_overrides_and_rejects_invalid_value() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file("config/default.toml", "[api]\nhttp_status = \"semantic\"")?;
            let loader = ConfigLoader::new("config", Environment::Test);
            assert_eq!(
                FrameworkConfig::default().api.http_status,
                HttpStatusMode::AlwaysOk
            );
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|e| figment::Error::from(e.to_string()))?
                    .api
                    .http_status,
                HttpStatusMode::Semantic
            );
            jail.set_env("APP_API__HTTP_STATUS", "always_ok");
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|e| figment::Error::from(e.to_string()))?
                    .api
                    .http_status,
                HttpStatusMode::AlwaysOk
            );
            jail.set_env("APP_API__HTTP_STATUS", "invalid");
            let error = loader
                .load_framework()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(
                error.to_ascii_lowercase().contains("api.http_status"),
                "{error}"
            );
            Ok(())
        });
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
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env(
                "APP_SERVER__CORS__ALLOWED_ORIGINS",
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
        let _environment_guard = crate::lock_test_env();
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

            jail.set_env("APP_DATABASE__URL", "postgres://prefixed/database");
            jail.set_env("APP_SERVER__PORT", "4200");
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
    fn environment_selection_uses_app_env_by_default() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("SHOP_ENV", "production");
            jail.set_env("OPENRUST_ENV", "production");
            assert_eq!(
                Environment::from_env().map_err(|error| figment::Error::from(error.to_string()))?,
                Environment::Development
            );
            jail.set_env("APP_ENV", "test");
            assert_eq!(
                Environment::from_env().map_err(|error| figment::Error::from(error.to_string()))?,
                Environment::Test
            );
            Ok(())
        });
    }

    #[test]
    fn custom_namespace_selects_config_dir_and_values_without_mixing_defaults() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("APP_SERVER__PORT", "4001");
            jail.set_env("SHOP_SERVER__PORT", "4002");
            jail.set_env("SHOP_ENV", "test");
            jail.set_env("SHOP_CONFIG_DIR", "shop-config");
            jail.create_dir("shop-config")?;
            jail.create_file("shop-config/default.toml", "[server]\nhost = \"0.0.0.0\"")?;

            let loader = ConfigLoader::from_env_with_namespace("SHOP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            let config = loader
                .load_framework()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.config_dir(), Path::new("shop-config"));
            assert_eq!(config.server.port, 4002);
            assert_eq!(config.server.host, "0.0.0.0");
            Ok(())
        });
    }

    #[test]
    fn custom_namespace_bootstraps_environment_from_dotenv() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("SHOP_ENV", "");
            jail.set_env("SHOP_SERVER__PORT", "");
            jail.set_env("APP_SERVER__PORT", "");
            jail.clear_env();
            jail.create_file(
                ".env",
                "SHOP_ENV=test\nSHOP_SERVER__PORT=4999\nAPP_SERVER__PORT=4111\n",
            )?;
            let env = dotenv_environment("SHOP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(env, Environment::Test);
            let _ = load_dotenv();
            let loader = ConfigLoader::from_environment_with_namespace(env, "SHOP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4999
            );
            Ok(())
        });
    }

    #[test]
    fn invalid_namespace_is_rejected_before_loading() {
        assert!(
            ConfigLoader::new("config", Environment::Test)
                .with_namespace("bad-")
                .is_err()
        );
        assert!(Environment::from_env_with_namespace("").is_err());
    }

    #[test]
    fn invalid_custom_environment_reports_selected_key() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("SHOP_ENV", "staging");
            assert!(matches!(
                Environment::from_env_with_namespace("SHOP_"),
                Err(ConfigError::UnknownEnvironment { key, value })
                    if key == "SHOP_ENV" && value == "staging"
            ));
            Ok(())
        });
    }
}
