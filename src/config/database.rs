//! PostgreSQL/MySQL connection and pool configuration.

use std::fmt;

use serde::Deserialize;

/// Database connection and pool settings from the `[database]` configuration section.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Database connection URL. `DATABASE_URL` and `APP_DATABASE__URL` map here.
    pub url: Option<String>,
    /// Optional read-only replica URLs. Each replica uses the same pool limits as the primary.
    pub read_replicas: Vec<String>,
    /// Maximum number of open pooled connections.
    pub max_connections: u32,
    /// Minimum number of open pooled connections.
    pub min_connections: u32,
    /// Maximum time to establish a connection, in seconds.
    pub connect_timeout_secs: u64,
    /// Maximum time to wait for an available pooled connection, in seconds.
    pub acquire_timeout_secs: u64,
    /// Prefix applied to logical framework table names unless a full-name mapping overrides it.
    pub table_prefix: String,
    /// Maps logical table names to complete physical names, without applying `table_prefix`.
    pub table_names: std::collections::BTreeMap<String, String>,
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
            .finish()
    }
}
