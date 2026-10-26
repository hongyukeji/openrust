//! SMS 发送配置。

use serde::Deserialize;

/// `[sms]` 区段的配置。
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct SmsConfig {
    /// 发送后端：默认使用 `log`；启用 `sms-twilio` 后可使用 `twilio`。
    pub driver: String,
    /// 用于 Messages resource path 的 Twilio account SID。
    pub account_sid: Option<String>,
    /// 作为 HTTP Basic 用户名的 Twilio API key SID。
    pub api_key_sid: Option<String>,
    /// 作为 HTTP Basic 密码的 Twilio API key secret。
    pub api_key_secret: Option<String>,
    /// Twilio 发送号码（E.164）；不能与 messaging service SID 同时设置。
    pub from: Option<String>,
    /// Twilio Messaging Service SID；不能与 `from` 同时设置。
    pub messaging_service_sid: Option<String>,
}

impl Default for SmsConfig {
    fn default() -> Self {
        Self {
            driver: "log".into(),
            account_sid: None,
            api_key_sid: None,
            api_key_secret: None,
            from: None,
            messaging_service_sid: None,
        }
    }
}

impl std::fmt::Debug for SmsConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmsConfig")
            .field("driver", &self.driver)
            .field(
                "account_sid",
                &self.account_sid.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "api_key_sid",
                &self.api_key_sid.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "api_key_secret",
                &self.api_key_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("from", &self.from.as_ref().map(|_| "[REDACTED]"))
            .field(
                "messaging_service_sid",
                &self.messaging_service_sid.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}
