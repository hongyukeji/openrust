//! SMS delivery configuration.

use serde::Deserialize;

/// Configuration for the `[sms]` section.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct SmsConfig {
    /// Delivery backend: `log` by default, or `twilio` with `sms-twilio`.
    pub driver: String,
    /// Twilio account SID used in the Messages resource path.
    pub account_sid: Option<String>,
    /// Twilio API key SID used as the HTTP Basic username.
    pub api_key_sid: Option<String>,
    /// Twilio API key secret used as the HTTP Basic password.
    pub api_key_secret: Option<String>,
    /// Twilio sender number (E.164); mutually exclusive with a messaging service SID.
    pub from: Option<String>,
    /// Twilio Messaging Service SID; mutually exclusive with `from`.
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
