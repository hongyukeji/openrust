//! 邮件投递默认值。

use openrust::config::{ConfigContext, ConfigError, MailConfig};

/// 默认记录邮件而不向外投递。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<MailConfig, ConfigError> {
    Ok(MailConfig::default())
}
