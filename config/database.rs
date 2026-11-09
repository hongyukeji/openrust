//! 默认连接、独立具名连接及连接池的应用默认值。

use openrust::config::{
    ConfigContext, ConfigError, DatabaseConfig, DatabaseInstanceConfig, DatabaseMigrationsConfig,
};
use std::collections::BTreeMap;

/// 只控制下方独立 reporting 连接是否进入默认配置，不影响部署来源的具名连接。
const REPORTING_ENABLED: bool = false;

/// URL 由统一 DB_* 环境层或显式来源提供，None 不内置真实连接或凭据。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<DatabaseConfig, ConfigError> {
    let instances = if REPORTING_ENABLED {
        BTreeMap::from([(
            "reporting".to_owned(),
            DatabaseInstanceConfig {
                // 启用前提供此独立实例的 URL；不自动继承默认连接。
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
                table_names: BTreeMap::new(),
            },
        )])
    } else {
        BTreeMap::new()
    };
    Ok(DatabaseConfig {
        migrations: DatabaseMigrationsConfig::default(),
        url: None,
        foreign_key_constraints: ctx.nullable_bool_or("DB_FOREIGN_KEYS", Some(true)),
        busy_timeout: None,
        journal_mode: None,
        synchronous: None,
        pragmas: serde_json::Map::new(),
        // 各副本使用相同池限制；空列表表示无只读副本。
        read_replicas: Vec::new(),
        max_connections: 10,
        min_connections: 1,
        connect_timeout_secs: 5,
        acquire_timeout_secs: 30,
        table_prefix: String::new(),
        // 完整物理表名映射优先于前缀，不再重复添加 table_prefix。
        table_names: BTreeMap::new(),
        instances,
    })
}
