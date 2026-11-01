//! 应用身份默认值。

use openrust::config::{AppConfig, ConfigContext, ConfigError};

/// 在同次 dotenv 快照上求值应用身份。
pub fn config(ctx: &ConfigContext) -> Result<AppConfig, ConfigError> {
    Ok(AppConfig {
        name: ctx.env_or("APP_NAME", "openrust"),
        debug: ctx.bool_or("APP_DEBUG", false)?,
        url: ctx.env_or("APP_URL", "http://localhost:3000"),
        ..AppConfig::default()
    })
}
