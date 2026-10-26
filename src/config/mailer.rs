//! Mail delivery configuration.

use serde::Deserialize;

/// Configuration for the `[mail]` section.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct MailConfig {
    /// Delivery backend: `log` by default, or `smtp` when `mail-smtp` is enabled.
    pub driver: String,
    /// SMTP server host name.
    pub smtp_host: Option<String>,
    /// SMTP server port.
    pub smtp_port: u16,
    /// Optional SMTP username; a username requires a password.
    pub smtp_username: Option<String>,
    /// Optional SMTP password; omitted from debug output.
    pub smtp_password: Option<String>,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            driver: "log".into(),
            smtp_host: None,
            smtp_port: 587,
            smtp_username: None,
            smtp_password: None,
        }
    }
}

impl std::fmt::Debug for MailConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MailConfig")
            .field("driver", &self.driver)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_username", &self.smtp_username)
            .field(
                "smtp_password",
                &self.smtp_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}
