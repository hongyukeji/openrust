//! 短信后端与 Twilio 的应用默认值。

use openrust::config::{ConfigContext, ConfigError, SmsConfig};

/// log 仅记录短信；twilio 还需 sms-twilio feature 和部署凭据。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<SmsConfig, ConfigError> {
    Ok(SmsConfig {
        driver: "log".to_owned(),
        // None 不提供 Twilio 身份或秘密，启用该 driver 前须配置。
        account_sid: None,
        api_key_sid: None,
        api_key_secret: None,
        // from 使用 E.164；与 messaging_service_sid 二选一，不能同时配置。
        from: None,
        messaging_service_sid: None,
    })
}
