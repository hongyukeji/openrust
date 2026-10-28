//! 邮件发送配置。

use serde::{Deserialize, Serialize};

/// `[mail]` 区段的配置。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MailConfig {
    /// 发送后端：默认使用 `log`；启用 `mail-smtp` 后可使用 `smtp`。
    pub driver: String,
    /// SMTP 服务器主机名。
    pub smtp_host: Option<String>,
    /// SMTP 服务器端口。
    pub smtp_port: u16,
    /// 是否要求 STARTTLS；仅未认证的本地回环开发 sink 可以关闭。
    pub smtp_starttls: bool,
    /// 可选的 SMTP 用户名；设置用户名时必须提供密码。
    pub smtp_username: Option<String>,
    /// 可选的 SMTP 密码；不会出现在 debug 输出中。
    pub smtp_password: Option<String>,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            driver: "log".into(),
            smtp_host: None,
            smtp_port: 587,
            smtp_starttls: true,
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
            .field("smtp_starttls", &self.smtp_starttls)
            .field("smtp_username", &self.smtp_username)
            .field(
                "smtp_password",
                &self.smtp_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}
