//! Passport 可选包的应用配置。

use openrust::config::{ConfigContext, ConfigError};
use openrust_passport::PassportConfig;

/// 通过应用的同一 dotenv 快照读取固定 Passport 的部署变量。
#[allow(clippy::unnecessary_wraps, reason = "配置清单统一使用可失败的配置函数")]
pub fn config(context: &ConfigContext) -> Result<PassportConfig, ConfigError> {
    Ok(PassportConfig {
        private_key: context.env("PASSPORT_PRIVATE_KEY"),
        public_key: context.env("PASSPORT_PUBLIC_KEY"),
        connection: context.env("PASSPORT_CONNECTION"),
        ..PassportConfig::default()
    })
}
