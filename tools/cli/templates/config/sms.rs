//! 短信发送默认值。

use openrust::config::{ConfigContext, ConfigError, SmsConfig};

/// 默认记录短信而不向外发送。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<SmsConfig, ConfigError> {
    Ok(SmsConfig::default())
}
