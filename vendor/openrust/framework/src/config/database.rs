//! 关系数据库连接、连接池及 SQLite 物理连接配置。

use std::fmt;

use serde::{Deserialize, Serialize};

/// `database.migrations` 迁移账本配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseMigrationsConfig {
    /// 迁移账本表名，默认 `migrations`。
    pub table: String,
}

impl Default for DatabaseMigrationsConfig {
    fn default() -> Self {
        Self {
            table: "migrations".to_owned(),
        }
    }
}

/// `[database]` 配置区段中的数据库连接和连接池设置。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// 应用与模块共用的迁移账本。
    pub migrations: DatabaseMigrationsConfig,
    /// 数据库连接 URL；可由 `DB_*` 分项设置生成，也可通过配置文件或 `APP_DATABASE_URL` 指定。
    pub url: Option<String>,
    /// SQLite 每条物理连接的外键约束；默认 `Some(true)`，`None` 保留原生或先行 PRAGMA。
    pub foreign_key_constraints: Option<bool>,
    /// SQLite 原始标量 `busy_timeout`（毫秒）；`None` 跳过，负数交原生引擎处理。
    pub busy_timeout: Option<serde_json::Value>,
    /// SQLite 原始标量 `journal_mode`；`None` 不强制切换文件日志模式。
    pub journal_mode: Option<serde_json::Value>,
    /// SQLite 原始标量 `synchronous`，支持数字级别；`None` 跳过。
    pub synchronous: Option<serde_json::Value>,
    /// SQLite 按声明顺序执行的其他标量 PRAGMA；专用配置在其后覆盖。
    pub pragmas: serde_json::Map<String, serde_json::Value>,
    /// 可选的只读副本 URL。每个副本使用与主库相同的连接池限制。
    pub read_replicas: Vec<String>,
    /// 连接池允许打开的最大连接数。
    pub max_connections: u32,
    /// 连接池维持的最少打开连接数。
    pub min_connections: u32,
    /// 建立连接的最长等待时间（秒）。
    pub connect_timeout_secs: u64,
    /// 等待可用池连接的最长时间（秒）。
    pub acquire_timeout_secs: u64,
    /// 应用于框架逻辑表名的前缀；若设置完整名称映射，则以映射为准。
    pub table_prefix: String,
    /// 将逻辑表名映射为完整物理表名，不再应用 `table_prefix`。
    pub table_names: std::collections::BTreeMap<String, String>,
    /// 具名独立连接；每项完整配置且不会继承默认连接设置。
    pub instances: std::collections::BTreeMap<String, DatabaseInstanceConfig>,
}

impl DatabaseConfig {
    /// 是否配置默认连接 URL；具名实例不代表默认连接可用。
    ///
    /// 此选择不验证 URL、驱动或连接健康；这些仍由数据库 Provider 统一检查。
    pub fn has_default_connection(&self) -> bool {
        self.url.is_some()
    }
}

/// `[database.instances.<name>]` 中独立连接池的配置。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseInstanceConfig {
    /// 此实例的数据库连接 URL。
    pub url: Option<String>,
    /// SQLite 每条物理连接的外键约束；默认 `Some(true)`，`None` 保留原生或先行 PRAGMA。
    pub foreign_key_constraints: Option<bool>,
    /// SQLite 原始标量 `busy_timeout`（毫秒）；`None` 跳过，负数交原生引擎处理。
    pub busy_timeout: Option<serde_json::Value>,
    /// SQLite 原始标量 `journal_mode`；`None` 不强制切换文件日志模式。
    pub journal_mode: Option<serde_json::Value>,
    /// SQLite 原始标量 `synchronous`，支持数字级别；`None` 跳过。
    pub synchronous: Option<serde_json::Value>,
    /// SQLite 按声明顺序执行的其他标量 PRAGMA；专用配置在其后覆盖。
    pub pragmas: serde_json::Map<String, serde_json::Value>,
    /// 此实例可选的只读副本 URL。
    pub read_replicas: Vec<String>,
    /// 此实例连接池的最大连接数。
    pub max_connections: u32,
    /// 此实例连接池的最小连接数。
    pub min_connections: u32,
    /// 此实例建立连接的最长等待时间（秒）。
    pub connect_timeout_secs: u64,
    /// 此实例等待可用池连接的最长时间（秒）。
    pub acquire_timeout_secs: u64,
    /// 此实例的框架逻辑表名前缀。
    pub table_prefix: String,
    /// 此实例的完整物理表名映射。
    pub table_names: std::collections::BTreeMap<String, String>,
}

impl Default for DatabaseInstanceConfig {
    fn default() -> Self {
        Self {
            url: None,
            foreign_key_constraints: Some(true),
            busy_timeout: None,
            journal_mode: None,
            synchronous: None,
            pragmas: serde_json::Map::new(),
            read_replicas: Vec::new(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 30,
            table_prefix: String::new(),
            table_names: std::collections::BTreeMap::new(),
        }
    }
}

impl std::fmt::Debug for DatabaseInstanceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseInstanceConfig")
            .field("url", &self.url.as_ref().map(|_| "[REDACTED]"))
            .field(
                "read_replicas",
                &format_args!("{} configured", self.read_replicas.len()),
            )
            .field("foreign_key_constraints", &self.foreign_key_constraints)
            .field(
                "busy_timeout",
                &self.busy_timeout.as_ref().map(|_| "[CONFIGURED]"),
            )
            .field("pragmas_count", &self.pragmas.len())
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("acquire_timeout_secs", &self.acquire_timeout_secs)
            .field("table_prefix", &self.table_prefix)
            .field("table_names", &self.table_names)
            .finish_non_exhaustive()
    }
}

impl From<&DatabaseConfig> for DatabaseInstanceConfig {
    fn from(config: &DatabaseConfig) -> Self {
        Self {
            url: config.url.clone(),
            foreign_key_constraints: config.foreign_key_constraints,
            busy_timeout: config.busy_timeout.clone(),
            journal_mode: config.journal_mode.clone(),
            synchronous: config.synchronous.clone(),
            pragmas: config.pragmas.clone(),
            read_replicas: config.read_replicas.clone(),
            max_connections: config.max_connections,
            min_connections: config.min_connections,
            connect_timeout_secs: config.connect_timeout_secs,
            acquire_timeout_secs: config.acquire_timeout_secs,
            table_prefix: config.table_prefix.clone(),
            table_names: config.table_names.clone(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            migrations: DatabaseMigrationsConfig::default(),
            url: None,
            foreign_key_constraints: Some(true),
            busy_timeout: None,
            journal_mode: None,
            synchronous: None,
            pragmas: serde_json::Map::new(),
            read_replicas: Vec::new(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 30,
            table_prefix: String::new(),
            table_names: std::collections::BTreeMap::new(),
            instances: std::collections::BTreeMap::new(),
        }
    }
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("url", &self.url.as_ref().map(|_| "[REDACTED]"))
            .field(
                "read_replicas",
                &format_args!("{} configured", self.read_replicas.len()),
            )
            .field("foreign_key_constraints", &self.foreign_key_constraints)
            .field(
                "busy_timeout",
                &self.busy_timeout.as_ref().map(|_| "[CONFIGURED]"),
            )
            .field("pragmas_count", &self.pragmas.len())
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("acquire_timeout_secs", &self.acquire_timeout_secs)
            .field("table_prefix", &self.table_prefix)
            .field("table_names", &self.table_names)
            .field("migrations", &self.migrations)
            .field("instances", &self.instances)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_connection_selection_does_not_infer_driver_or_named_pool_health() {
        let mut config = DatabaseConfig::default();
        assert!(!config.has_default_connection());
        config.instances.insert(
            "reporting".into(),
            DatabaseInstanceConfig {
                url: Some("mysql://private.invalid/named".into()),
                ..DatabaseInstanceConfig::default()
            },
        );
        assert!(!config.has_default_connection());
        config
            .read_replicas
            .push("postgres://private.invalid/replica".into());
        assert!(!config.has_default_connection());
        // 非法但显式配置的URL不能在能力选择时静默当作未配置；后续统一预检必须拒绝。
        config.url = Some(String::new());
        assert!(config.has_default_connection());
        config.url = Some("unknown://private.invalid/default".into());
        assert!(config.has_default_connection());
    }
}
