//! 分层配置。
//!
//! 配置定义位于 `config` 下各领域模块，并由唯一的 [`ConfigLoader`] 合并。
//! 运行时 TOML 文件保存在应用根目录的 `config/` 中。
//!
//! 配置源按以下顺序合并，后面的层会覆盖前面的层：
//!
//! 1. crate 默认值（[`FrameworkConfig::default`]）
//! 2. `<config-dir>/default.toml`
//! 3. `<config-dir>/<env>.toml`，其中 `<env>` 来自 `APP_ENV`（默认值为 `development`）
//! 4. 非生产环境从本地 `.env` 加载的变量，然后是进程变量；变量以 `APP_` 为前缀，并使用 `__` 表示嵌套
//!    （`APP_SERVER__PORT=4000` → `server.port`）
//!
//! 若进程未设置环境，bootstrap 只会从 `.env` 读取 `APP_ENV` 以选择运行环境。
//! 生产环境不会加载 `.env` 文件。已有进程变量的优先级高于 `.env` 中的值。

use std::fmt;
use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub mod api;
pub mod app;
pub mod auth;
pub mod cache;
pub mod database;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod logger;
pub mod mailer;
#[cfg(feature = "queue")]
pub mod queue;
#[cfg(feature = "encryption")]
pub mod security;
pub mod server;
pub mod signing;
pub mod sms;
pub mod storage;

pub use api::{ApiConfig, HttpStatusMode};
pub use app::{AppConfig, Environment};
pub use auth::{JwtConfig, PasswordConfig, SessionConfig};
pub use cache::{CacheConfig, CacheSingleflightConfig, CacheStoreConfig, CacheSwrConfig};
pub use database::{DatabaseConfig, DatabaseInstanceConfig};
#[cfg(feature = "grpc")]
pub use grpc::GrpcConfig;
pub use logger::{LogConfig, LogFormat};
pub use mailer::MailConfig;
#[cfg(feature = "queue")]
pub use queue::{QueueConfig, QueueDriverKind};
#[cfg(feature = "encryption")]
pub use security::EncryptionConfig;
pub use server::{CompressionConfig, CorsConfig, ServerConfig};
pub use signing::SignedLinksConfig;
pub use sms::SmsConfig;
pub use storage::{StorageConfig, StorageDiskConfig};

/// 用于选择运行环境的环境变量。
pub const ENV_VAR: &str = "APP_ENV";
/// 用于选择配置目录的环境变量。
pub const CONFIG_DIR_VAR: &str = "APP_CONFIG_DIR";
/// 覆盖配置键的环境变量前缀。
pub const ENV_PREFIX: &str = "APP_";
/// 相对于工作目录、用于保存 `default.toml` 和环境专属文件的目录。
pub const DEFAULT_CONFIG_DIR: &str = "config";

/// 配置错误。仅可能在 bootstrap 期间发生。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// 值缺失、类型错误或文件无法解析。
    /// 错误消息保留安全的键路径和错误类别，但省略源值。
    #[error("{0}")]
    Source(ConfigSourceError),
    /// 所选命名空间的环境变量包含不受支持的值。
    #[error("unknown environment `{value}` in {key}; expected development, test or production")]
    UnknownEnvironment {
        /// 提供无效值的环境变量键。
        key: String,
        /// 无法识别的环境变量值。
        value: String,
    },
    /// 配置值已加载，但违反框架约束。
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

/// 已移除秘密值的配置源错误。
#[derive(Clone)]
pub struct ConfigSourceError {
    message: String,
    missing: bool,
}

impl ConfigSourceError {
    /// 返回源错误是否由字段缺失导致。
    pub fn missing(&self) -> bool {
        self.missing
    }
}

impl fmt::Debug for ConfigSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigSourceError")
            .field("message", &self.message)
            .field("missing", &self.missing)
            .finish()
    }
}

impl fmt::Display for ConfigSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConfigSourceError {}

impl From<figment::Error> for ConfigError {
    fn from(source: figment::Error) -> Self {
        let missing = source.missing();
        let message = source
            .clone()
            .into_iter()
            .map(safe_figment_error)
            .collect::<Vec<_>>()
            .join("; ");
        Self::Source(ConfigSourceError { message, missing })
    }
}

fn safe_figment_error(error: figment::Error) -> String {
    use figment::error::Kind;

    let path = if error.path.is_empty() {
        "configuration".to_owned()
    } else {
        error.path.join(".")
    };
    let detail = match error.kind {
        Kind::Message(_) => "invalid value".to_owned(),
        Kind::InvalidType(_, expected) => format!("invalid type; expected {expected}"),
        Kind::InvalidValue(_, expected) => format!("invalid value; expected {expected}"),
        Kind::InvalidLength(length, expected) => {
            format!("invalid length {length}; expected {expected}")
        }
        Kind::UnknownVariant(_, expected) => {
            format!("unknown variant; expected one of {}", expected.join(", "))
        }
        Kind::UnknownField(_, expected) => {
            format!("unknown field; expected one of {}", expected.join(", "))
        }
        Kind::MissingField(field) => format!("missing field `{field}`"),
        Kind::DuplicateField(field) => format!("duplicate field `{field}`"),
        Kind::ISizeOutOfRange(_) | Kind::USizeOutOfRange(_) => "integer out of range".into(),
        Kind::Unsupported(_) => "unsupported configuration value".into(),
        Kind::UnsupportedKey(_, expected) => format!("unsupported map key; expected {expected}"),
    };
    format!("{path}: {detail}")
}

/// 由框架管理的配置。应用可在旁边添加自己的配置区段。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameworkConfig {
    /// `[app]` 区段。
    pub app: AppConfig,
    /// `[server]` 区段。
    pub server: ServerConfig,
    /// `[api]` 区段。
    pub api: ApiConfig,
    /// `[log]` 区段。
    pub log: LogConfig,
    /// `I18nPlugin` 使用的 `[i18n]` locale 默认值。
    pub i18n: crate::i18n::I18nConfig,
    /// `[grpc]` 区段；仅在启用 `grpc` feature 时存在。
    #[cfg(feature = "grpc")]
    pub grpc: GrpcConfig,
}

/// 无自有配置的应用使用的标记类型：`type Config = NoConfig;`。
///
/// 不能使用单元类型 `()`，因为 serde 不会将 map 反序列化为单元值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoConfig {}

/// 若工作目录中存在 `.env` 文件则加载，并返回其路径。
///
/// 不会覆盖现有环境变量。
pub fn load_dotenv() -> Option<PathBuf> {
    dotenvy::dotenv().ok()
}

/// 只从 `.env` 读取所选命名空间的环境键，不修改进程变量。
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

/// 为一个运行环境构建合并后的配置源。
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    config_dir: PathBuf,
    env: Environment,
    namespace: String,
}

impl ConfigLoader {
    /// 使用 `APP_CONFIG_DIR`（或 [`DEFAULT_CONFIG_DIR`]）和 `APP_ENV`。
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with_namespace(ENV_PREFIX)
    }

    /// 使用所选命名空间的 `<PREFIX>ENV` 和 `<PREFIX>CONFIG_DIR` 值。
    pub fn from_env_with_namespace(namespace: &str) -> Result<Self, ConfigError> {
        let env = Environment::from_env_with_namespace(namespace)?;
        Self::from_environment_with_namespace(env, namespace)
    }

    /// 使用所选命名空间的 `<PREFIX>CONFIG_DIR`（或 [`DEFAULT_CONFIG_DIR`]）。
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

    /// 使用显式指定的目录和运行环境。
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

    /// 返回只读取所选应用命名空间的加载器。
    ///
    /// 命名空间必须是以 `_` 结尾的大写标识符，例如 `SHOP_`。
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Result<Self, ConfigError> {
        let namespace = namespace.into();
        validate_namespace(&namespace)?;
        self.namespace = namespace;
        Ok(self)
    }

    /// 所选运行环境。
    pub fn environment(&self) -> Environment {
        self.env
    }

    /// 读取 TOML 文件的目录。
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// 提取所有配置区段所用的分层 [`Figment`]。
    pub fn figment(&self) -> Figment {
        Figment::from(Serialized::defaults(FrameworkConfig::default()))
            .merge(Toml::file(self.config_dir.join("default.toml")))
            .merge(Toml::file(
                self.config_dir.join(format!("{}.toml", self.env)),
            ))
            .merge(
                Env::raw()
                    .only(&["DATABASE_URL", "REDIS_URL", "PORT"])
                    .map(|key| {
                        if key == "PORT" {
                            "server.port".into()
                        } else if key == "REDIS_URL" {
                            "cache.stores.default.dsn".into()
                        } else {
                            "database.url".into()
                        }
                    })
                    .split("."),
            )
            .merge(
                Env::prefixed(&self.namespace)
                    .filter(|key| key != "ENV" && key != "CONFIG_DIR")
                    .map(|key| {
                        if key == "REDIS_URL" {
                            "cache.stores.default.dsn".into()
                        } else {
                            key.into()
                        }
                    })
                    .split("__"),
            )
    }

    /// 将完整的合并配置提取为 `T`。
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, ConfigError> {
        Ok(self.figment().extract()?)
    }

    /// 提取框架配置区段，并将 `app.env` 固定为所选运行环境。
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

/// 将键名看似秘密的值（`password`、`secret`、`token`、`key`、`dsn`）替换为 `********`。
pub fn redact_secrets(value: &mut serde_json::Value) {
    const SECRET_MARKERS: [&str; 5] = ["password", "secret", "token", "key", "dsn"];
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let lowered = key.to_ascii_lowercase();
                if SECRET_MARKERS.iter().any(|m| lowered.contains(m)) {
                    redact_secret_values(child);
                } else {
                    redact_secrets(child);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

fn redact_secret_values(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for child in map.values_mut() {
                redact_secret_values(child);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_secret_values),
        _ => *value = serde_json::Value::String("********".to_owned()),
    }
}

#[cfg(test)]
// `figment::Jail` 闭包必须返回 `figment::Result`；其错误类型按设计较大。
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
            "auth": { "jwt": { "active_key_id": "v1", "keys": { "v1": "secret-key", "old": "old-secret" } } },
            "app": { "name": "Blog" }
        });
        redact_secrets(&mut value);
        assert_eq!(value["database"]["password"], "********");
        assert_eq!(value["mail"]["smtp"]["token"], "********");
        assert_eq!(value["auth"]["jwt"]["active_key_id"], "********");
        assert_eq!(value["auth"]["jwt"]["keys"]["v1"], "********");
        assert_eq!(value["auth"]["jwt"]["keys"]["old"], "********");
        assert_eq!(value["app"]["name"], "Blog");
    }

    #[test]
    fn configuration_source_errors_do_not_echo_invalid_string_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let secret = "source-error-secret-must-not-leak";
        let figment = Figment::new().merge(("security.encryption.keys", secret));
        let Err(source) = figment.extract_inner::<u64>("security.encryption.keys") else {
            return Err(std::io::Error::other("deserialization unexpectedly succeeded").into());
        };
        let error = ConfigError::from(source);
        assert!(error.to_string().contains("security.encryption.keys"));
        assert!(!error.to_string().contains(secret));
        assert!(!format!("{error:?}").contains(secret));
        Ok(())
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

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_toml_loads_server_and_named_client_configuration() -> Result<(), toml::de::Error> {
        let parsed: FrameworkConfig = toml::from_str(
            "[grpc]\nlisten_addr = \"127.0.0.1:6060\"\ndefault_client = \"billing\"\n\n[grpc.clients]\nbilling = \"http://127.0.0.1:6061\"",
        )?;
        assert_eq!(parsed.grpc.listen_addr.to_string(), "127.0.0.1:6060");
        assert_eq!(parsed.grpc.default_client.as_deref(), Some("billing"));
        assert_eq!(
            parsed.grpc.clients.get("billing").map(String::as_str),
            Some("http://127.0.0.1:6061")
        );
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
