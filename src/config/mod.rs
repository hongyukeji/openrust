//! 分层配置。
//!
//! 配置定义位于 `config` 下各领域模块，并由唯一的 [`ConfigLoader`] 合并。
//! 运行时 TOML 文件保存在应用根目录的 `config/` 中。
//!
//! 配置源按以下顺序合并，后面的层会覆盖前面的层：
//!
//! 1. crate 默认值（[`FrameworkConfig::default`]）
//! 2. `<config-dir>/**/*.toml` 中按相对目录与文件名映射后的完整键自然排序的领域配置
//! 3. Laravel 风格的 `DB_*` / `REDIS_*` 分项连接设置
//! 4. 本地 `.env` 与进程变量中的应用配置；默认以 `APP_` 为前缀，使用单下划线扁平键
//!    （`APP_PORT=4000` → `server.port`；`APP_NAME` → `app.name`）
//!
//! 若进程未设置环境，bootstrap 只会从 `.env` 读取 `APP_ENV` 以选择运行环境。
//! 所有环境都可从应用工作目录的 `.env` 加载变量；已有进程变量的优先级高于 `.env` 中的值。

/// Provider 配置仓储、显式 schema 与启动合并操作。
pub mod repository;
mod sources;
pub use sources::ConfigSources;
pub(crate) use sources::natural_name_cmp;
use sources::{CapturedProvider, EnvironmentSnapshot};

use std::fmt;
use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Serialized};
use figment::{Metadata, Provider};
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
#[cfg(feature = "realtime")]
pub mod realtime;
#[cfg(feature = "encryption")]
pub mod security;
pub mod server;
pub mod session;
pub mod signing;
pub mod sms;
pub mod storage;
#[cfg(feature = "view")]
pub mod view;

pub use api::{ApiConfig, HttpStatusMode};
pub use app::{AppConfig, Environment};
pub use auth::{JwtConfig, PasswordConfig};
pub use cache::{CacheConfig, CacheSingleflightConfig, CacheStoreConfig, CacheSwrConfig};
pub use database::{DatabaseConfig, DatabaseInstanceConfig};
#[cfg(feature = "grpc")]
pub use grpc::{GrpcClientConfig, GrpcClientTlsConfig, GrpcConfig, GrpcServerTlsConfig};
pub use logger::{LogConfig, LogFormat};
pub use mailer::MailConfig;
#[cfg(feature = "queue")]
pub use queue::{QueueConfig, QueueConnectionConfig, QueueDriverKind};
#[cfg(feature = "realtime")]
pub use realtime::RealtimeConfig;
#[cfg(feature = "encryption")]
pub use security::EncryptionConfig;
pub use server::{CompressionConfig, CorsConfig, ServerConfig};
pub use session::SessionConfig;
pub use signing::SignedLinksConfig;
pub use sms::SmsConfig;
pub use storage::{StorageConfig, StorageDiskConfig};
#[cfg(feature = "view")]
pub use view::ViewConfig;

/// 用于选择运行环境的环境变量。
pub const ENV_VAR: &str = "APP_ENV";
/// 用于选择配置目录的环境变量。
pub const CONFIG_DIR_VAR: &str = "APP_CONFIG_DIR";
/// 覆盖配置键的环境变量前缀。
pub const ENV_PREFIX: &str = "APP_";
/// 相对于工作目录、用于保存按领域划分的 TOML 配置文件的目录。
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
    /// `[realtime]` 区段；仅在启用 `realtime` feature 时存在。
    #[cfg(feature = "realtime")]
    pub realtime: RealtimeConfig,
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

/// 启动时加载应用工作目录中的 `.env`；文件缺失时继续启动，文件无效时安全失败。
pub(crate) fn load_dotenv_for_bootstrap() -> Result<(), ConfigError> {
    match dotenvy::from_path(".env") {
        Ok(()) => Ok(()),
        Err(error) if error.not_found() => Ok(()),
        Err(_) => Err(ConfigError::Invalid(
            "无法加载 .env 文件，请检查文件格式和读取权限".to_owned(),
        )),
    }
}

/// 只从 `.env` 读取所选命名空间的环境键，不修改进程变量。
pub(crate) fn dotenv_environment(namespace: &str) -> Result<Environment, ConfigError> {
    validate_namespace(namespace)?;
    let key = format!("{namespace}ENV");
    let entries = match dotenvy::from_path_iter(".env") {
        Ok(entries) => entries,
        Err(error) if error.not_found() => return Ok(Environment::Development),
        Err(_) => {
            return Err(ConfigError::Invalid(
                "无法读取 .env 文件，请检查文件格式和读取权限".to_owned(),
            ));
        }
    };
    for entry in entries {
        let (entry_key, entry_value) = entry.map_err(|_| {
            ConfigError::Invalid(".env 文件格式无效，请检查变量定义语法".to_owned())
        })?;
        if entry_key == key && !entry_value.trim().is_empty() {
            return entry_value
                .parse()
                .map_err(|error: ConfigError| match error {
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
#[derive(Clone)]
pub struct ConfigLoader {
    config_dir: PathBuf,
    explicit_config_dir: bool,
    config_dir_key: String,
    env: Environment,
    namespace: String,
}

impl fmt::Debug for ConfigLoader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigLoader")
            .field("environment", &self.env)
            .field("namespace", &self.namespace)
            .field("config_dir_source", &self.config_dir_source())
            .finish_non_exhaustive()
    }
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
        let selected = std::env::var_os(&config_dir_key);
        if selected.as_ref().is_some_and(|value| value.is_empty()) {
            return Err(ConfigError::Invalid(format!(
                "{config_dir_key}: 配置目录不能为空"
            )));
        }
        Ok(Self {
            explicit_config_dir: selected.is_some(),
            config_dir: selected.map_or_else(|| PathBuf::from(DEFAULT_CONFIG_DIR), PathBuf::from),
            config_dir_key,
            env,
            namespace: namespace.to_owned(),
        })
    }

    /// 使用显式指定的目录和运行环境；目录在 `figment` 或 `load` 时验证。
    pub fn new(config_dir: impl Into<PathBuf>, env: Environment) -> Self {
        Self {
            config_dir: config_dir.into(),
            explicit_config_dir: true,
            config_dir_key: "config_dir".to_owned(),
            env,
            namespace: ENV_PREFIX.to_owned(),
        }
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

    pub(crate) fn config_dir_source(&self) -> &str {
        if self.explicit_config_dir {
            &self.config_dir_key
        } else {
            "default"
        }
    }

    /// 构建同一次捕获的有序原始来源与 Figment 投影。
    #[allow(
        clippy::too_many_lines,
        reason = "顺序列出文件、通用别名与命名空间环境层，避免隐藏配置优先级"
    )]
    pub fn sources(&self) -> Result<ConfigSources, ConfigError> {
        let defaults = serde_json::to_value(FrameworkConfig::default())
            .map_err(|_| ConfigError::Invalid("configuration: 默认值无法表示为有序配置".into()))?;
        let entries = defaults
            .as_object()
            .ok_or_else(|| ConfigError::Invalid("configuration: 默认值根必须是对象".into()))?;
        let mut sources = ConfigSources::from_ordered_entries(entries.clone())?;
        for (name, file) in self.domain_files()? {
            let content = std::fs::read_to_string(&file).map_err(|_| {
                ConfigError::Invalid(format!(
                    "{}: 配置文件无法读取（源值未公开）",
                    self.config_dir_key
                ))
            })?;
            let table: toml::Table = toml::from_str(&content).map_err(|_| {
                ConfigError::Invalid(format!(
                    "{}: TOML 配置无效（源值未公开）",
                    self.config_dir_key
                ))
            })?;
            let ordered = serde_json::to_value(table).map_err(|_| {
                ConfigError::Invalid(format!(
                    "{}: TOML 值无法转换（源值未公开）",
                    self.config_dir_key
                ))
            })?;
            sources.merge_file(
                &name,
                ordered,
                Metadata::named("TOML file").source(file.as_path()),
            )?;
        }
        let environment = EnvironmentSnapshot::capture();
        if let Some(url) = conventional_database_url(&environment) {
            let layer = serde_json::json!({ "database": { "url": url } });
            sources.merge(Serialized::defaults(layer.clone()), layer)?;
        }
        if let Some(dsn) = conventional_redis_dsn(&environment) {
            let layer = serde_json::json!({ "cache": { "stores": { "default": { "dsn": dsn } } } });
            sources.merge(Serialized::defaults(layer.clone()), layer)?;
        }
        #[cfg(feature = "queue")]
        if let Some(redis_url) = conventional_redis_dsn(&environment) {
            let layer = serde_json::json!({ "queue": { "redis_url": redis_url } });
            sources.merge(Serialized::defaults(layer.clone()), layer)?;
        }
        let common = environment.layer(common_environment_path)?;
        let common_provider = CapturedProvider::new(&Serialized::defaults(common.clone()))?
            .with_metadata(Env::prefixed("").metadata());
        sources.merge(common_provider, common)?;
        let common_nulls = session_environment_nulls(&environment, "", true);
        sources.merge(Serialized::defaults(common_nulls.clone()), common_nulls)?;
        let namespaced =
            environment.layer(|key| namespaced_environment_path(key, &self.namespace))?;
        let namespaced_provider = CapturedProvider::new(&Serialized::defaults(namespaced.clone()))?
            .with_metadata(Env::prefixed(&self.namespace).metadata());
        sources.merge(namespaced_provider, namespaced)?;
        let namespaced_nulls = session_environment_nulls(&environment, &self.namespace, false);
        sources.merge(
            Serialized::defaults(namespaced_nulls.clone()),
            namespaced_nulls,
        )?;
        Ok(sources)
    }

    /// 返回有损的 Figment 视图；来源原始键序需使用 [`Self::sources`]。
    pub fn figment(&self) -> Result<Figment, ConfigError> {
        Ok(self.sources()?.figment().clone())
    }

    fn domain_files(&self) -> Result<Vec<(String, PathBuf)>, ConfigError> {
        let root = match std::fs::canonicalize(&self.config_dir) {
            Ok(root) => root,
            Err(error)
                if !self.explicit_config_dir
                    && error.kind() == std::io::ErrorKind::NotFound
                    && std::fs::symlink_metadata(&self.config_dir)
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(Vec::new());
            }
            Err(_) => return Err(self.directory_error()),
        };
        let mut files = std::collections::HashMap::new();
        self.visit_config_directory(&root, &root, &mut files)?;
        let mut files = files.into_iter().collect::<Vec<_>>();
        files.sort_by(|(left, _), (right, _)| natural_name_cmp(left, right));
        Ok(files)
    }

    fn visit_config_directory(
        &self,
        root: &Path,
        directory: &Path,
        files: &mut std::collections::HashMap<String, PathBuf>,
    ) -> Result<(), ConfigError> {
        let entries = std::fs::read_dir(directory).map_err(|_| self.directory_error())?;
        for entry in entries {
            let path = entry.map_err(|_| self.directory_error())?.path();
            let entry_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| self.directory_error())?;
            if entry_name.starts_with('.') {
                continue;
            }
            let kind = std::fs::symlink_metadata(&path).map_err(|_| self.directory_error())?;
            if kind.file_type().is_dir() {
                self.visit_config_directory(root, &path, files)?;
            } else if (kind.file_type().is_file() || kind.file_type().is_symlink())
                && path
                    .extension()
                    .is_some_and(|extension| extension == "toml")
            {
                let real = std::fs::canonicalize(&path).map_err(|_| self.directory_error())?;
                if !real.is_file() {
                    continue;
                }
                let basename = real
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| self.directory_error())?;
                let stem = basename.strip_suffix(".toml").unwrap_or(basename);
                let parent = directory
                    .strip_prefix(root)
                    .map_err(|_| self.directory_error())?;
                let mut parts = parent
                    .components()
                    .map(|component| {
                        component
                            .as_os_str()
                            .to_str()
                            .ok_or_else(|| self.directory_error())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                parts.push(stem);
                files.insert(parts.join("."), real);
            }
        }
        Ok(())
    }

    fn directory_error(&self) -> ConfigError {
        ConfigError::Invalid(format!(
            "{}: 配置目录不存在、不是目录或无法完整读取（源值未公开）",
            self.config_dir_key
        ))
    }

    /// 将完整的合并配置提取为 `T`。
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, ConfigError> {
        Ok(self.figment()?.extract()?)
    }

    /// 提取框架配置区段，并将 `app.env` 固定为所选运行环境。
    pub fn load_framework(&self) -> Result<FrameworkConfig, ConfigError> {
        let mut config: FrameworkConfig = self.load()?;
        config.app.env = self.env;
        Ok(config)
    }
}

// Laravel env 将未加引号的 null/(null) 视为 null；Figment 默认仍把它当字符串。
// 只修正本簇声明的 nullable Session 字段，且保留每层环境源的通常优先级。
fn session_environment_nulls(
    environment: &EnvironmentSnapshot,
    namespace: &str,
    conventional: bool,
) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    for (suffix, field) in [
        ("DRIVER", "driver"),
        ("COOKIE", "cookie"),
        ("DOMAIN", "domain"),
        (
            if conventional {
                "SECURE_COOKIE"
            } else {
                "SECURE"
            },
            "secure",
        ),
        ("SAME_SITE", "same_site"),
    ] {
        if environment
            .get(&format!("{namespace}SESSION_{suffix}"))
            .is_some_and(|value| {
                value.eq_ignore_ascii_case("null") || value.eq_ignore_ascii_case("(null)")
            })
        {
            fields.insert(field.into(), serde_json::Value::Null);
        }
    }
    if fields.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({"session":fields})
    }
}

fn common_environment_path(key: &str) -> Option<String> {
    const KEYS: &[&str] = &[
        "PORT",
        "DB_TABLE_PREFIX",
        "REDIS_DEFAULT_STORE",
        "QUEUE_DRIVER",
        "MAIL_DRIVER",
        "MAIL_SMTP_HOST",
        "MAIL_SMTP_PORT",
        "MAIL_SMTP_STARTTLS",
        "MAIL_SMTP_USERNAME",
        "MAIL_SMTP_PASSWORD",
        "SMS_ACCOUNT_SID",
        "SMS_API_KEY_SID",
        "SMS_API_KEY_SECRET",
        "SMS_FROM",
        "SMS_MESSAGING_SERVICE_SID",
        "SMS_DRIVER",
        "I18N_DEFAULT_LOCALE",
        "SESSION_DRIVER",
        "SESSION_COOKIE",
        "SESSION_LIFETIME",
        "SESSION_EXPIRE_ON_CLOSE",
        "SESSION_PATH",
        "SESSION_DOMAIN",
        "SESSION_SECURE_COOKIE",
        "SESSION_HTTP_ONLY",
        "SESSION_SAME_SITE",
        "SESSION_PARTITIONED_COOKIE",
    ];
    if !KEYS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
    {
        return None;
    }
    Some(
        match key {
            "PORT" => "server.port",
            "DB_TABLE_PREFIX" => "database.table_prefix",
            "REDIS_DEFAULT_STORE" => "cache.default_store",
            "QUEUE_DRIVER" => "queue.driver",
            "MAIL_DRIVER" => "mail.driver",
            "MAIL_SMTP_HOST" => "mail.smtp_host",
            "MAIL_SMTP_PORT" => "mail.smtp_port",
            "MAIL_SMTP_STARTTLS" => "mail.smtp_starttls",
            "MAIL_SMTP_USERNAME" => "mail.smtp_username",
            "MAIL_SMTP_PASSWORD" => "mail.smtp_password",
            "SMS_DRIVER" => "sms.driver",
            "SMS_ACCOUNT_SID" => "sms.account_sid",
            "SMS_API_KEY_SID" => "sms.api_key_sid",
            "SMS_API_KEY_SECRET" => "sms.api_key_secret",
            "SMS_FROM" => "sms.from",
            "SMS_MESSAGING_SERVICE_SID" => "sms.messaging_service_sid",
            "I18N_DEFAULT_LOCALE" => "i18n.default_locale",
            "SESSION_DRIVER" => "session.driver",
            "SESSION_COOKIE" => "session.cookie",
            "SESSION_LIFETIME" => "session.lifetime",
            "SESSION_EXPIRE_ON_CLOSE" => "session.expire_on_close",
            "SESSION_PATH" => "session.path",
            "SESSION_DOMAIN" => "session.domain",
            "SESSION_SECURE_COOKIE" => "session.secure",
            "SESSION_HTTP_ONLY" => "session.http_only",
            "SESSION_SAME_SITE" => "session.same_site",
            "SESSION_PARTITIONED_COOKIE" => "session.partitioned",
            other => return Some(other.to_ascii_lowercase()),
        }
        .to_owned(),
    )
}

fn namespaced_environment_path(key: &str, namespace: &str) -> Option<String> {
    let prefix = key.get(..namespace.len())?;
    if !prefix.eq_ignore_ascii_case(namespace) {
        return None;
    }
    let name = key.get(namespace.len()..)?;
    if name.eq_ignore_ascii_case("ENV")
        || name.eq_ignore_ascii_case("CONFIG_DIR")
        || name.eq_ignore_ascii_case("SERVER_PORT")
        || name.contains("__")
    {
        return None;
    }
    Some(flat_environment_path(name))
}

fn flat_environment_path(key: &str) -> String {
    match key {
        "PORT" => "server.port".to_owned(),
        "NAME" => "app.name".to_owned(),
        "URL" => "app.url".to_owned(),
        "TIMEZONE" => "app.timezone".to_owned(),
        "SERVER_CORS_ENABLED" => "server.cors.enabled".to_owned(),
        "SERVER_CORS_ALLOWED_ORIGINS" => "server.cors.allowed_origins".to_owned(),
        "AUTH_JWT_KEYS_LOCAL" => "auth.jwt.keys.local".to_owned(),
        "SECURITY_SIGNED_LINKS_KEYS_LOCAL" => "security.signed_links.keys.local".to_owned(),
        "SECURITY_ENCRYPTION_ACTIVE_KEY_ID" => "security.encryption.active_key_id".to_owned(),
        "SECURITY_ENCRYPTION_KEYS_LOCAL" => "security.encryption.keys.local".to_owned(),
        "SECURITY_ENCRYPTION_KEYS_TEST" => "security.encryption.keys.test".to_owned(),
        "GRPC_ALLOW_INSECURE_SERVER" => "grpc.allow_insecure_server".to_owned(),
        key if key.starts_with("CACHE_STORES_") => {
            let Some((store, field)) = key["CACHE_STORES_".len()..].split_once('_') else {
                return key.to_ascii_lowercase().replace('_', ".");
            };
            format!(
                "cache.stores.{}.{}",
                store.to_ascii_lowercase(),
                field.to_ascii_lowercase()
            )
        }
        _ => {
            let Some((section, field)) = key.split_once('_') else {
                return format!("app.{}", key.to_ascii_lowercase());
            };
            format!(
                "{}.{}",
                section.to_ascii_lowercase(),
                field.to_ascii_lowercase()
            )
        }
    }
}

fn environment_value(environment: &EnvironmentSnapshot, key: &str) -> Option<String> {
    environment.value(key)
}

fn conventional_database_url(environment: &EnvironmentSnapshot) -> Option<String> {
    const KEYS: [&str; 6] = [
        "DB_CONNECTION",
        "DB_HOST",
        "DB_PORT",
        "DB_DATABASE",
        "DB_USERNAME",
        "DB_PASSWORD",
    ];
    if !KEYS.iter().any(|key| environment.present(key)) {
        return None;
    }

    let connection =
        environment_value(environment, "DB_CONNECTION").unwrap_or_else(|| "postgres".to_owned());
    let (scheme, default_port) = match connection.as_str() {
        "postgres" => ("postgres", 5432),
        "mysql" => ("mysql", 3306),
        _ => return Some("invalid://".to_owned()),
    };
    let Ok(mut url) = url::Url::parse(&format!("{scheme}://localhost")) else {
        return Some("invalid://".to_owned());
    };
    let host = environment_value(environment, "DB_HOST").unwrap_or_else(|| "127.0.0.1".to_owned());
    if url.set_host(Some(&host)).is_err() {
        return Some("invalid://".to_owned());
    }
    let port =
        environment_value(environment, "DB_PORT").unwrap_or_else(|| default_port.to_string());
    if port.parse::<u16>().is_err() || url.set_port(port.parse::<u16>().ok()).is_err() {
        return Some("invalid://".to_owned());
    }
    if let Some(username) = environment_value(environment, "DB_USERNAME")
        && url.set_username(&username).is_err()
    {
        return Some("invalid://".to_owned());
    }
    if let Some(password) = environment_value(environment, "DB_PASSWORD")
        && url.set_password(Some(&password)).is_err()
    {
        return Some("invalid://".to_owned());
    }
    let database =
        environment_value(environment, "DB_DATABASE").unwrap_or_else(|| "openrust".to_owned());
    if url
        .path_segments_mut()
        .map(|mut segments| {
            segments.pop_if_empty();
            segments.push(&database);
        })
        .is_err()
    {
        return Some("invalid://".to_owned());
    }
    Some(url.into())
}

fn conventional_redis_dsn(environment: &EnvironmentSnapshot) -> Option<String> {
    const KEYS: [&str; 6] = [
        "REDIS_HOST",
        "REDIS_PASSWORD",
        "REDIS_USERNAME",
        "REDIS_PORT",
        "REDIS_DB",
        "REDIS_SCHEME",
    ];
    if !KEYS.iter().any(|key| environment.present(key)) {
        return None;
    }

    let scheme =
        environment_value(environment, "REDIS_SCHEME").unwrap_or_else(|| "redis".to_owned());
    if !matches!(scheme.as_str(), "redis" | "rediss") {
        return Some("invalid://".to_owned());
    }
    let Ok(mut url) = url::Url::parse(&format!("{scheme}://localhost")) else {
        return Some("invalid://".to_owned());
    };
    let host =
        environment_value(environment, "REDIS_HOST").unwrap_or_else(|| "127.0.0.1".to_owned());
    if url.set_host(Some(&host)).is_err() {
        return Some("invalid://".to_owned());
    }
    let port = environment_value(environment, "REDIS_PORT").unwrap_or_else(|| "6379".to_owned());
    if port.parse::<u16>().is_err() || url.set_port(port.parse::<u16>().ok()).is_err() {
        return Some("invalid://".to_owned());
    }
    if let Some(username) = environment_value(environment, "REDIS_USERNAME")
        && url.set_username(&username).is_err()
    {
        return Some("invalid://".to_owned());
    }
    if let Some(password) = environment_value(environment, "REDIS_PASSWORD")
        .filter(|password| !password.eq_ignore_ascii_case("null"))
        && url.set_password(Some(&password)).is_err()
    {
        return Some("invalid://".to_owned());
    }
    let database = environment_value(environment, "REDIS_DB").unwrap_or_else(|| "0".to_owned());
    if database.parse::<u32>().is_err()
        || url
            .path_segments_mut()
            .map(|mut segments| {
                segments.pop_if_empty();
                segments.push(&database);
            })
            .is_err()
    {
        return Some("invalid://".to_owned());
    }
    Some(url.into())
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
    fn explicit_config_directories_fail_closed_without_exposing_values() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            let loader = ConfigLoader::new("SECRET_MISSING_PATH", Environment::Test);
            assert!(!format!("{loader:?}").contains("SECRET_MISSING_PATH"));
            let result = loader.load_framework();
            assert!(result.is_err(), "显式路径缺失不能静默使用默认值");
            let error = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(error.contains("config_dir"));
            assert!(!error.contains("SECRET_MISSING_PATH"));

            jail.set_env("APP_CONFIG_DIR", "SECRET_MISSING_PATH");
            let result = ConfigLoader::from_env().and_then(|loader| loader.load_framework());
            assert!(result.is_err());
            let error = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(error.contains("APP_CONFIG_DIR"));
            assert!(!error.contains("SECRET_MISSING_PATH"));

            jail.set_env("APP_CONFIG_DIR", "");
            assert!(ConfigLoader::from_env().is_err(), "空的显式路径不是未声明");
            jail.create_file("not-a-directory", "secret-content")?;
            assert!(
                ConfigLoader::new("not-a-directory", Environment::Test)
                    .load_framework()
                    .is_err()
            );
            jail.set_env("SHOP_CONFIG_DIR", "SECRET_MISSING_PATH");
            let error = ConfigLoader::from_env_with_namespace("SHOP_")
                .and_then(|loader| loader.load_framework())
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(error.contains("SHOP_CONFIG_DIR"));
            assert!(!error.contains("SECRET_MISSING_PATH"));
            Ok(())
        });
    }

    #[test]
    fn missing_implicit_default_directory_and_existing_empty_directory_are_valid() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            let loader =
                ConfigLoader::from_environment_with_namespace(Environment::Test, "OPTIONAL_")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
            assert!(loader.load_framework().is_ok());
            jail.create_dir("empty-config")?;
            assert!(
                ConfigLoader::new("empty-config", Environment::Test)
                    .load_framework()
                    .is_ok()
            );
            Ok(())
        });
    }

    #[test]
    fn discovered_invalid_domain_file_reports_safe_failure() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file(
                "config/application.toml",
                "[app]\nname = 'SECRET_UNTERMINATED",
            )?;
            let result = ConfigLoader::new("config", Environment::Test).load_framework();
            assert!(result.is_err());
            let error = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(!error.contains("SECRET_UNTERMINATED"));
            Ok(())
        });
    }

    #[test]
    fn a_directory_named_toml_is_traversed_as_a_directory() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config/not-a-file.toml")?;
            jail.create_file("config/not-a-file.toml/child.toml", "value = 1")?;
            let loader = ConfigLoader::new("config", Environment::Test);
            assert_eq!(
                loader
                    .domain_files()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .len(),
                1
            );
            Ok(())
        });
    }

    #[cfg(unix)]
    #[test]
    fn broken_default_directory_symlink_is_an_error() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|_| {
            std::os::unix::fs::symlink("missing-target", "config")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            let loader =
                ConfigLoader::from_environment_with_namespace(Environment::Test, "OPTIONAL_")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
            assert!(loader.figment().is_err());
            Ok(())
        });
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_is_rejected_when_permissions_are_enforced() {
        use std::os::unix::fs::PermissionsExt;
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            let directory = jail.create_dir("private-config")?;
            let original = std::fs::metadata(&directory)
                .map_err(|error| figment::Error::from(error.to_string()))?
                .permissions();
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o000))
                .map_err(|error| figment::Error::from(error.to_string()))?;
            let enforced = std::fs::read_dir(&directory).is_err();
            let result = ConfigLoader::new(&directory, Environment::Test).figment();
            std::fs::set_permissions(&directory, original)
                .map_err(|error| figment::Error::from(error.to_string()))?;
            if enforced {
                assert!(result.is_err());
            } else {
                eprintln!("跳过权限拒绝断言：当前进程可绕过目录读取权限");
            }
            Ok(())
        });
    }

    #[test]
    fn domain_modules_preserve_capability_config_paths() {
        #[cfg(feature = "auth")]
        {
            let _: Option<crate::auth::JwtConfig> = None::<auth::JwtConfig>;
            let _: Option<crate::auth::PasswordConfig> = None::<auth::PasswordConfig>;
            let _: Option<crate::auth::SessionConfig> = None::<SessionConfig>;
        }
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        {
            let _: Option<crate::db::DatabaseConfig> = None::<database::DatabaseConfig>;
        }
        let _: Option<crate::mail::MailConfig> = None::<mailer::MailConfig>;
    }

    #[test]
    fn domain_files_override_defaults_and_accept_environment_names() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file(
                "config/server.toml",
                "port = 4000\nhost = \"0.0.0.0\"\npublic_dir = \"config-public\"\nroot_target = \"file:/home.html\"",
            )?;
            jail.create_file("config/test.toml", "value = 5000")?;
            jail.set_env("APP_SERVER_HOST", "10.0.0.1");
            jail.set_env("APP_SERVER_PUBLIC_DIR", "env-public");
            jail.set_env("APP_SERVER_ROOT_TARGET", "redirect:/welcome");
            jail.set_env("APP_NAME", "shop");
            jail.set_env("APP_URL", "https://example.test");

            let loader = ConfigLoader::new("config", Environment::Test);
            let config = loader
                .load_framework()
                .map_err(|e| figment::Error::from(e.to_string()))?;
            assert_eq!(
                config.server.port, 4000,
                "领域文件按文件名命名且合法的 test.toml 不跳过"
            );
            assert_eq!(config.server.host, "10.0.0.1", "env var overrides files");
            assert_eq!(config.server.public_dir, PathBuf::from("env-public"));
            assert_eq!(config.server.root_target, "redirect:/welcome");
            assert_eq!(config.app.name, "shop");
            assert_eq!(config.app.url, "https://example.test");
            let sources = loader
                .sources()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            let repository = repository::ConfigRepository::new(sources);
            assert_eq!(
                repository
                    .get("test")
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                Some(serde_json::json!({"value": 5000}))
            );
            assert_eq!(
                config.app.env,
                Environment::Test,
                "env is pinned by the loader"
            );
            Ok(())
        });
    }

    // 文件命名空间按完整键自然序载入，文件内声明顺序与环境覆盖保持原位。
    #[test]
    fn ordered_sources_keep_file_namespace_and_environment_precedence() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_file("config/part10.toml", "last = 10\nshared = 10\n")?;
            jail.create_file(
                "config/part2.toml",
                "z = 1\na = 2\nshared = 2\nitems = [9, 8]\n",
            )?;
            jail.set_env("APP_PART2_A", "3");
            let sources = ConfigLoader::new("config", Environment::Test)
                .sources()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            let repository = repository::ConfigRepository::new(sources.clone());
            let raw = repository
                .all()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(
                raw.as_object().map(|items| items
                    .keys()
                    .take(5)
                    .map(String::as_str)
                    .collect::<Vec<_>>()),
                Some(vec!["app", "server", "api", "log", "i18n"])
            );
            let ordered = repository
                .get("part2")
                .map_err(|error| figment::Error::from(error.to_string()))?
                .ok_or_else(|| figment::Error::from("ordered section missing"))?;
            assert_eq!(
                ordered
                    .as_object()
                    .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
                Some(vec!["z", "a", "shared", "items"])
            );
            assert_eq!(ordered["a"], 3);
            assert_eq!(ordered["shared"], 2);
            assert_eq!(ordered["items"], serde_json::json!([9, 8]));
            assert_eq!(
                repository
                    .get("part10.shared")
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                Some(serde_json::json!(10))
            );
            assert_eq!(sources.figment().extract_inner::<u32>("part2.a")?, 3);
            assert_eq!(
                sources
                    .figment()
                    .find_metadata("part2.z")
                    .map(|metadata| metadata.name.as_ref()),
                Some("TOML file")
            );
            Ok(())
        });
    }

    #[test]
    fn recursive_namespaces_follow_oracle_natural_order_and_ignore_hidden_entries() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_dir("config/pkg")?;
            jail.create_dir("config/pkg/deep")?;
            jail.create_dir("config/.hidden")?;
            jail.create_file("config/part10.toml", "value = 10")?;
            jail.create_file("config/part2.toml", "value = 2")?;
            jail.create_file("config/pkg/item10.toml", "value = 10")?;
            jail.create_file("config/pkg/item2.toml", "value = 2")?;
            jail.create_file("config/pkg/deep/x.toml", "value = 1")?;
            jail.create_file("config/.dot.toml", "value = 0")?;
            jail.create_file("config/.hidden/inside.toml", "value = 0")?;
            let loader = ConfigLoader::new("config", Environment::Test);
            let names = loader
                .domain_files()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            assert_eq!(
                names,
                ["part2", "part10", "pkg.deep.x", "pkg.item2", "pkg.item10"]
            );
            let sources = loader
                .sources()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(
                sources
                    .figment()
                    .find_metadata("pkg.deep.x.value")
                    .map(|metadata| metadata.name.as_ref()),
                Some("TOML file")
            );
            let repository = repository::ConfigRepository::new(sources);
            assert_eq!(
                repository
                    .get("pkg.deep.x.value")
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                Some(serde_json::json!(1))
            );
            assert_eq!(
                repository
                    .get("pkg")
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                Some(
                    serde_json::json!({"deep": {"x": {"value": 1}}, "item2": {"value": 2}, "item10": {"value": 10}})
                )
            );
            repository
                .set("pkg.deep.x.value", serde_json::json!(5))
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(
                repository
                    .get("pkg.deep.x.value")
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                Some(serde_json::json!(5))
            );
            Ok(())
        });
    }

    #[allow(
        clippy::too_many_lines,
        reason = "同一 Provider 生命周期需核对文件来源、环境覆盖与 set 后的一致性"
    )]
    #[tokio::test]
    async fn exact_file_sections_keep_shallow_provider_defaults_through_boot_and_set()
    -> crate::core::Result<()> {
        use crate::core::{
            BootContext, BoxFuture, ProviderBuilder, RegisterContext, ServiceProvider,
        };
        use repository::SectionSchema;

        struct DefaultsProbe {
            file_source: bool,
        }

        impl ServiceProvider for DefaultsProbe {
            fn register(&self, app: &mut RegisterContext<'_>) -> crate::core::Result<()> {
                app.config_section(
                    "fixture",
                    serde_json::json!({"label": "base", "nested": {"old": 1}}),
                    SectionSchema::any(),
                    |_| Ok(()),
                )?;
                app.config_section(
                    "cache",
                    serde_json::json!({"stores": {"array": {"driver": "array"}}, "nested": {"old": 1}}),
                    SectionSchema::any(),
                    |_| Ok(()),
                )
            }

            fn boot<'a>(
                &'a self,
                app: &'a mut BootContext<'_>,
            ) -> BoxFuture<'a, crate::core::Result<()>> {
                Box::pin(async move {
                    let fixture: serde_json::Value = app.section("fixture")?;
                    let expected = if self.file_source {
                        serde_json::json!({"new": 2})
                    } else {
                        serde_json::json!({"old": 1, "new": 2})
                    };
                    assert_eq!(fixture["nested"], expected);
                    Ok(())
                })
            }
        }

        struct CacheOnly;
        impl ServiceProvider for CacheOnly {
            fn register(&self, app: &mut RegisterContext<'_>) -> crate::core::Result<()> {
                app.config_section(
                    "cache",
                    serde_json::json!({"before": "base", "nested": {"old": 1}, "custom": {"old": 1}}),
                    SectionSchema::any(),
                    |_| Ok(()),
                )
            }
        }

        let directory = tempfile::tempdir().map_err(crate::core::Error::internal)?;
        std::fs::write(directory.path().join("fixture.toml"), "[nested]\nnew = 2\n")
            .map_err(crate::core::Error::internal)?;
        std::fs::write(
            directory.path().join("cache.toml"),
            "[stores.redis]\ndriver = 'redis'\n[nested]\nnew = 2\n",
        )
        .map_err(crate::core::Error::internal)?;
        let mut sources = ConfigLoader::new(directory.path(), Environment::Test).sources()?;
        let environment = serde_json::json!({
            "cache": {"stores": {"redis": {"dsn": "redis://environment"}}, "optional": null}
        });
        sources.merge(Serialized::defaults(environment.clone()), environment)?;
        let profile = sources.figment().profile().clone();
        let mut builder = ProviderBuilder::new();
        builder.provider(DefaultsProbe { file_source: true });
        let runtime = builder.start(sources).await?;
        let repository = runtime.config();
        assert_eq!(
            repository.get("fixture")?,
            Some(serde_json::json!({"label": "base", "nested": {"new": 2}}))
        );
        assert_eq!(repository.get("fixture.nested.old")?, None);
        assert_eq!(
            repository.get("cache.stores.array.driver")?,
            Some(serde_json::json!("array"))
        );
        assert_eq!(
            repository.get("cache.stores.redis.driver")?,
            Some(serde_json::json!("redis"))
        );
        assert_eq!(
            repository.get("cache.stores.redis.dsn")?,
            Some(serde_json::json!("redis://environment"))
        );
        assert_eq!(
            repository.get("cache.optional")?,
            Some(serde_json::Value::Null)
        );
        assert_eq!(
            repository.get("cache.nested")?,
            Some(serde_json::json!({"new": 2}))
        );
        assert_eq!(repository.snapshot().profile(), &profile);
        assert_eq!(
            repository
                .snapshot()
                .find_metadata("fixture.nested.new")
                .map(|metadata| metadata.name.as_ref()),
            Some("TOML file")
        );
        repository.set("fixture.nested.new", serde_json::json!(4))?;
        assert_eq!(
            repository.get("fixture.nested")?,
            Some(serde_json::json!({"new": 4}))
        );
        assert_eq!(
            repository.section::<serde_json::Value>("fixture")?["nested"],
            serde_json::json!({"new": 4})
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;

        let programmatic = ConfigSources::from_ordered_entries([
            ("fixture", serde_json::json!({"nested": {"new": 2}})),
            (
                "cache",
                serde_json::json!({"stores": {"redis": {"driver": "redis"}}, "nested": {"new": 2}}),
            ),
        ])?;
        let mut builder = ProviderBuilder::new();
        builder.provider(DefaultsProbe { file_source: false });
        let runtime = builder.start(programmatic).await?;
        assert_eq!(
            runtime.config().get("fixture.nested")?,
            Some(serde_json::json!({"old": 1, "new": 2}))
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;

        let nested = tempfile::tempdir().map_err(crate::core::Error::internal)?;
        std::fs::create_dir(nested.path().join("cache")).map_err(crate::core::Error::internal)?;
        std::fs::write(nested.path().join("cache/custom.toml"), "new = 2\n")
            .map_err(crate::core::Error::internal)?;
        let mut child_sources = ConfigLoader::new(nested.path(), Environment::Test).sources()?;
        let child_environment = serde_json::json!({"cache": {"before": "environment"}});
        child_sources.merge(
            Serialized::defaults(child_environment.clone()),
            child_environment,
        )?;
        let child_profile = child_sources.figment().profile().clone();
        let environment_metadata = child_sources
            .figment()
            .find_metadata("cache.before")
            .map(|metadata| metadata.name.to_string());
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheOnly);
        let runtime = builder.start(child_sources).await?;
        assert_eq!(
            runtime.config().get("cache.nested.old")?,
            Some(serde_json::json!(1))
        );
        assert_eq!(
            runtime.config().get("cache.custom")?,
            Some(serde_json::json!({"old": 1}))
        );
        assert_eq!(
            runtime.config().get("cache.custom.old")?,
            Some(serde_json::json!(1))
        );
        assert_eq!(runtime.config().get("cache.custom.new")?, None);
        assert_eq!(
            runtime.config().get("cache.before")?,
            Some(serde_json::json!("environment"))
        );
        assert_eq!(runtime.config().snapshot().profile(), &child_profile);
        assert_eq!(
            runtime
                .config()
                .snapshot()
                .find_metadata("cache.before")
                .map(|metadata| metadata.name.to_string()),
            environment_metadata
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn provider_mergeable_options_append_and_renumber_file_arrays() -> crate::core::Result<()>
    {
        use crate::core::{
            BootContext, BoxFuture, ProviderBuilder, RegisterContext, ServiceProvider,
        };
        use repository::SectionSchema;

        struct ArrayProbe {
            defaults: serde_json::Value,
            expected: serde_json::Value,
        }

        impl ServiceProvider for ArrayProbe {
            fn register(&self, app: &mut RegisterContext<'_>) -> crate::core::Result<()> {
                app.config_section(
                    "broadcasting",
                    self.defaults.clone(),
                    SectionSchema::any(),
                    |_| Ok(()),
                )
            }

            fn boot<'a>(
                &'a self,
                app: &'a mut BootContext<'_>,
            ) -> BoxFuture<'a, crate::core::Result<()>> {
                Box::pin(async move {
                    let connections: serde_json::Value = app.section("broadcasting.connections")?;
                    assert_eq!(connections, self.expected);
                    Ok(())
                })
            }
        }

        for (defaults, file, expected) in [
            (
                serde_json::json!({"connections": [{"driver": "base"}]}),
                "connections = [{ driver = 'file' }]\n",
                serde_json::json!([{"driver": "base"}, {"driver": "file"}]),
            ),
            (
                serde_json::json!({"connections": {"0": {"driver": "base"}, "named": {"driver": "keep"}}}),
                "connections = { '7' = { driver = 'file' }, named = { driver = 'override' } }\n",
                serde_json::json!({"0": {"driver": "base"}, "named": {"driver": "override"}, "1": {"driver": "file"}}),
            ),
        ] {
            let directory = tempfile::tempdir().map_err(crate::core::Error::internal)?;
            std::fs::write(directory.path().join("broadcasting.toml"), file)
                .map_err(crate::core::Error::internal)?;
            let mut builder = ProviderBuilder::new();
            builder.provider(ArrayProbe {
                defaults,
                expected: expected.clone(),
            });
            let runtime = builder
                .start(ConfigLoader::new(directory.path(), Environment::Test).sources()?)
                .await?;
            assert_eq!(
                runtime.config().get("broadcasting.connections")?,
                Some(expected.clone())
            );
            assert_eq!(
                runtime
                    .config()
                    .section::<serde_json::Value>("broadcasting.connections")?,
                expected
            );
            runtime
                .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
                .await?;
        }
        Ok(())
    }

    #[test]
    fn colliding_mapped_keys_keep_the_last_file_seen_by_directory_iteration() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_dir("config/a")?;
            jail.create_file("config/a/b.toml", "value = 'nested'")?;
            jail.create_file("config/a.b.toml", "value = 'flat'")?;
            let loader = ConfigLoader::new("config", Environment::Test);
            let files = loader
                .domain_files()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].0, "a.b");
            let chosen = std::fs::read_to_string(&files[0].1)
                .map_err(|error| figment::Error::from(error.to_string()))?;
            let repository = repository::ConfigRepository::new(
                loader
                    .sources()
                    .map_err(|error| figment::Error::from(error.to_string()))?,
            );
            let expected = if chosen.contains("flat") {
                "flat"
            } else {
                "nested"
            };
            assert_eq!(
                repository
                    .get("a.b.value")
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                Some(serde_json::json!(expected))
            );
            Ok(())
        });
    }

    #[cfg(unix)]
    #[test]
    fn file_links_use_real_basename_while_directory_links_are_not_followed() {
        use std::os::unix::fs::symlink;
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.create_dir("config")?;
            jail.create_dir("config/real-dir")?;
            jail.create_dir("outside")?;
            jail.create_file("config/real-dir/entry.toml", "value = 1")?;
            jail.create_file("outside/origin.txt", "value = 2")?;
            symlink("real-dir", "config/alias-dir")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            symlink("../outside/origin.txt", "config/alias.toml")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            symlink("config", "linked-config")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            let loader = ConfigLoader::new("linked-config", Environment::Test);
            let names = loader
                .domain_files()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            assert_eq!(names, ["origin.txt", "real-dir.entry"]);
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
            jail.create_file("custom/server.toml", "port = 4321")?;
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
            jail.create_file("config/server.toml", "port = 4567")?;
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
            jail.create_file("config/server.toml", "port = \"not-a-port\"")?;
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
            jail.create_dir("default_config")?;
            jail.create_file("default_config/api.toml", "")?;
            jail.create_dir("config")?;
            jail.create_file("config/api.toml", "http_status = \"always_ok\"")?;
            let default_loader = ConfigLoader::new("default_config", Environment::Test);
            assert_eq!(
                default_loader
                    .load_framework()
                    .map_err(|e| figment::Error::from(e.to_string()))?
                    .api
                    .http_status,
                HttpStatusMode::Semantic
            );
            let loader = ConfigLoader::new("config", Environment::Test);
            assert_eq!(
                FrameworkConfig::default().api.http_status,
                HttpStatusMode::Semantic
            );
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|e| figment::Error::from(e.to_string()))?
                    .api
                    .http_status,
                HttpStatusMode::AlwaysOk
            );
            jail.set_env("APP_API_HTTP_STATUS", "semantic");
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|e| figment::Error::from(e.to_string()))?
                    .api
                    .http_status,
                HttpStatusMode::Semantic
            );
            jail.set_env("APP_API_HTTP_STATUS", "invalid");
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
            "[grpc]\nlisten_addr = \"127.0.0.1:6060\"\ndefault_client = \"billing\"\n\n[grpc.clients.billing]\nendpoint = \"http://127.0.0.1:6061\"",
        )?;
        assert_eq!(parsed.grpc.listen_addr.to_string(), "127.0.0.1:6060");
        assert_eq!(parsed.grpc.default_client.as_deref(), Some("billing"));
        assert_eq!(
            parsed
                .grpc
                .clients
                .get("billing")
                .map(|client| client.endpoint.as_str()),
            Some("http://127.0.0.1:6061")
        );
        Ok(())
    }

    #[test]
    fn cors_list_environment_override_uses_figment_array_syntax() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env(
                "APP_SERVER_CORS_ALLOWED_ORIGINS",
                "[\"https://example.com\", \"https://other.example\"]",
            );
            jail.create_dir("empty-config")?;
            let config = ConfigLoader::new("empty-config", Environment::Test)
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
    fn application_namespace_values_map_to_sections_and_override_defaults() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("PORT", "4100");
            jail.create_dir("empty-config")?;
            let loader = ConfigLoader::new("empty-config", Environment::Test);
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4100
            );

            jail.set_env("APP_SERVER_PORT", "4999");
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4100
            );

            jail.set_env("APP_DATABASE_URL", "postgres://custom/database");
            jail.set_env("APP_PORT", "4200");
            jail.set_env("APP_LOG_LEVEL", "debug");
            jail.set_env("APP_TIMEZONE", "Asia/Shanghai");
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .app
                    .timezone,
                "Asia/Shanghai"
            );
            let database: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("database")?;
            assert_eq!(database["url"], "postgres://custom/database");
            let log: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("log")?;
            assert_eq!(log["level"], "debug");
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
    fn laravel_style_database_and_redis_variables_build_connection_urls() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.set_env("DB_CONNECTION", "mysql");
            jail.set_env("DB_HOST", "db.example.test");
            jail.set_env("DB_DATABASE", "shop");
            jail.set_env("DB_USERNAME", "app user");
            jail.set_env("DB_PASSWORD", "p@ss:word");
            jail.set_env("REDIS_HOST", "cache.example.test");
            jail.set_env("REDIS_PASSWORD", "redis:secret");
            jail.set_env("REDIS_PORT", "6380");
            jail.set_env("REDIS_DB", "4");
            jail.set_env("DB_TABLE_PREFIX", "shop_");
            jail.set_env("REDIS_DEFAULT_STORE", "shared");
            jail.set_env("MAIL_DRIVER", "smtp");
            jail.set_env("MAIL_SMTP_HOST", "mail.example.test");
            jail.set_env("SMS_DRIVER", "log");
            jail.set_env("I18N_DEFAULT_LOCALE", "zh-CN");
            #[cfg(feature = "queue")]
            jail.set_env("QUEUE_DRIVER", "redis");

            jail.create_dir("empty-config")?;
            let loader = ConfigLoader::new("empty-config", Environment::Test);
            let database: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("database")?;
            let database_url = url::Url::parse(database["url"].as_str().unwrap_or_default())
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(database_url.scheme(), "mysql");
            assert_eq!(database_url.host_str(), Some("db.example.test"));
            assert_eq!(database_url.path(), "/shop");
            assert_eq!(database_url.username(), "app%20user");
            assert_eq!(database_url.password(), Some("p%40ss%3Aword"));
            assert_eq!(database["table_prefix"], "shop_");

            let cache: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("cache")?;
            let redis_url = url::Url::parse(
                cache["stores"]["default"]["dsn"]
                    .as_str()
                    .unwrap_or_default(),
            )
            .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(redis_url.host_str(), Some("cache.example.test"));
            assert_eq!(redis_url.port(), Some(6380));
            assert_eq!(redis_url.path(), "/4");
            assert_eq!(redis_url.password(), Some("redis%3Asecret"));
            assert_eq!(cache["default_store"], "shared");
            let mail: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("mail")?;
            assert_eq!(mail["driver"], "smtp");
            assert_eq!(mail["smtp_host"], "mail.example.test");
            let sms: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("sms")?;
            assert_eq!(sms["driver"], "log");
            let i18n: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("i18n")?;
            assert_eq!(i18n["default_locale"], "zh-CN");
            #[cfg(feature = "queue")]
            {
                let queue: serde_json::Value = loader
                    .figment()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .extract_inner("queue")?;
                assert_eq!(queue["driver"], "redis");
                let queue_url = url::Url::parse(queue["redis_url"].as_str().unwrap_or_default())
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(queue_url.host_str(), Some("cache.example.test"));
                assert_eq!(queue_url.path(), "/4");
                assert_eq!(queue_url.password(), Some("redis%3Asecret"));
            }
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
            jail.set_env("APP_PORT", "4001");
            jail.set_env("SHOP_PORT", "4002");
            jail.set_env("SHOP_ENV", "test");
            jail.set_env("SHOP_CONFIG_DIR", "shop-config");
            jail.create_dir("shop-config")?;
            jail.create_file("shop-config/server.toml", "host = \"0.0.0.0\"")?;

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
            jail.set_env("SHOP_PORT", "");
            jail.set_env("APP_PORT", "");
            jail.clear_env();
            jail.create_file(".env", "SHOP_ENV=test\nSHOP_PORT=4999\nAPP_PORT=4111\n")?;
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
