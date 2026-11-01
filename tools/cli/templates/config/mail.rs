//! 邮件发送及 SMTP 的应用默认值。

use openrust::config::{ConfigContext, ConfigError, MailConfig};

/// log 仅记录邮件；smtp 还需 mail-smtp feature 和真实服务配置。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<MailConfig, ConfigError> {
    Ok(MailConfig {
        driver: "log".to_owned(),
        // None 不指定 SMTP 主机；smtp driver 必须设置 Some 主机或部署来源。
        smtp_host: None,
        smtp_port: 587,
        // 仅未认证的本地回环开发 sink 可以关闭 STARTTLS。
        smtp_starttls: true,
        // None 表示不认证；用户名与密码必须成对提供，秘密留在部署来源。
        smtp_username: None,
        smtp_password: None,
    })
}
