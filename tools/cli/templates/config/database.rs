//! 数据库连接默认值。

use openrust::config::{ConfigContext, ConfigError, DatabaseConfig};

/// 保留逻辑表名前缀；数据库 URL 由统一环境层读取。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<DatabaseConfig, ConfigError> {
    Ok(DatabaseConfig::default())
}
