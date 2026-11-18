use std::path::Path;

use openrust::{
    config::{ConfigError, Environment, Value},
    http::Uri,
};
use serde_json::json;

use super::env::{get, path};

pub(super) fn load(
    base_path: &Path,
    env: &Environment,
    app_url: &str,
) -> Result<Value, ConfigError> {
    let app_url = app_url.parse::<Uri>().ok();
    let domain = json!(app_url.as_ref().and_then(Uri::host));
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 默认邮件传输
        |--------------------------------------------------------------------------
        |
        | MAIL_MAILER 选择传输。log 只记录邮件，实际投递需配置可用的投递传输。
        |
        */
        "default": get(env, "MAIL_MAILER", "log")?,

        /*
        |--------------------------------------------------------------------------
        | 邮件传输配置
        |--------------------------------------------------------------------------
        |
        | SMTP 地址和凭据使用 MAIL_*，第三方邮件 API 的凭据见 services.rs。
        | failover 依次尝试传输，roundrobin 在多个传输之间轮流选择。
        |
        */
        "mailers": {
            "smtp": {
                "transport": "smtp",
                "scheme": get(env, "MAIL_SCHEME", Value::Null)?,
                "url": get(env, "MAIL_URL", Value::Null)?,
                "host": get(env, "MAIL_HOST", "127.0.0.1")?,
                "port": get(env, "MAIL_PORT", 2525)?,
                "username": get(env, "MAIL_USERNAME", Value::Null)?,
                "password": get(env, "MAIL_PASSWORD", Value::Null)?,
                "timeout": null,
                "local_domain": get(env, "MAIL_EHLO_DOMAIN", domain)?,
            },
            "ses": {"transport": "ses"},
            "ses-v2": {"transport": "ses-v2"},
            "cloudflare": {"transport": "cloudflare"},
            "mailgun": {"transport": "mailgun"},
            "postmark": {"transport": "postmark"},
            "resend": {"transport": "resend"},
            "sendmail": {
                "transport": "sendmail",
                "path": get(env, "MAIL_SENDMAIL_PATH", "/usr/sbin/sendmail -bs -i")?,
            },
            "log": {"transport": "log", "channel": get(env, "MAIL_LOG_CHANNEL", Value::Null)?},
            "array": {"transport": "array"},
            "failover": {"transport": "failover", "mailers": ["smtp", "log"], "retry_after": 60},
            "roundrobin": {"transport": "roundrobin", "mailers": ["ses", "postmark"], "retry_after": 60},
        },

        /*
        |--------------------------------------------------------------------------
        | 默认发件人
        |--------------------------------------------------------------------------
        |
        | 通过 MAIL_FROM_ADDRESS 和 MAIL_FROM_NAME 设置。SMTP 登录账号由 MAIL_USERNAME 单独配置。
        |
        */
        "from": {
            "address": get(env, "MAIL_FROM_ADDRESS", "hello@example.com")?,
            "name": get(env, "MAIL_FROM_NAME", "Example")?,
        },

        /*
        |--------------------------------------------------------------------------
        | Markdown 邮件模板
        |--------------------------------------------------------------------------
        |
        | MAIL_MARKDOWN_THEME 选择主题，paths 指定自定义邮件组件的查找目录。
        |
        */
        "markdown": {
            "theme": get(env, "MAIL_MARKDOWN_THEME", "default")?,
            "paths": [path(&base_path.join("resources/views/vendor/mail"))?],
            "extensions": [],
            "secured_encoding": false,
        },
    }))
}
