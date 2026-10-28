//! PostgreSQL/MySQL 连接与连接池配置。

use std::fmt;

use serde::{Deserialize, Serialize};

/// `[database]` 配置区段中的数据库连接和连接池设置。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// 数据库连接 URL；可由 `DB_*` 分项设置生成，也可通过配置文件或 `APP_DATABASE_URL` 指定。
    pub url: Option<String>,
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

/// `[database.instances.<name>]` 中独立连接池的配置。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseInstanceConfig {
    /// 此实例的数据库连接 URL。
    pub url: Option<String>,
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
            read_replicas: Vec::new(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 30,
            table_prefix: "tb_".to_owned(),
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
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("acquire_timeout_secs", &self.acquire_timeout_secs)
            .field("table_prefix", &self.table_prefix)
            .field("table_names", &self.table_names)
            .finish()
    }
}

impl From<&DatabaseConfig> for DatabaseInstanceConfig {
    fn from(config: &DatabaseConfig) -> Self {
        Self {
            url: config.url.clone(),
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
            url: None,
            read_replicas: Vec::new(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 30,
            table_prefix: "tb_".to_owned(),
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
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("acquire_timeout_secs", &self.acquire_timeout_secs)
            .field("table_prefix", &self.table_prefix)
            .field("table_names", &self.table_names)
            .field("instances", &self.instances)
            .finish()
    }
}
