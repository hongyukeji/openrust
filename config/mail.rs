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
        "default": get(env, "MAIL_MAILER", "log")?,
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
        "from": {
            "address": get(env, "MAIL_FROM_ADDRESS", "hello@example.com")?,
            "name": get(env, "MAIL_FROM_NAME", "Example")?,
        },
        "markdown": {
            "theme": get(env, "MAIL_MARKDOWN_THEME", "default")?,
            "paths": [path(&base_path.join("resources/views/vendor/mail"))?],
            "extensions": [],
        },
    }))
}
