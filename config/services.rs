use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 第三方服务凭据
    |--------------------------------------------------------------------------
    |
    | 通过环境变量配置邮件供应商、Slack 通知与 GitHub 登录所需的凭据。
    |
    */
    Ok(json!({
        "github": {
            "client_id": get(env, "GITHUB_CLIENT_ID", Value::Null)?,
            "client_secret": get(env, "GITHUB_CLIENT_SECRET", Value::Null)?,
            "redirect": get(env, "GITHUB_REDIRECT_URI", Value::Null)?,
        },
        "postmark": {"token": get(env, "POSTMARK_TOKEN", Value::Null)?},
        "cloudflare": {
            "account_id": get(env, "CLOUDFLARE_ACCOUNT_ID", Value::Null)?,
            "key": get(env, "CLOUDFLARE_KEY", Value::Null)?,
        },
        "mailgun": {
            "domain": get(env, "MAILGUN_DOMAIN", Value::Null)?,
            "secret": get(env, "MAILGUN_SECRET", Value::Null)?,
            "endpoint": get(env, "MAILGUN_ENDPOINT", Value::Null)?,
        },
        "resend": {"key": get(env, "RESEND_KEY", Value::Null)?},
        "ses": {
            "key": get(env, "AWS_ACCESS_KEY_ID", Value::Null)?,
            "secret": get(env, "AWS_SECRET_ACCESS_KEY", Value::Null)?,
            "token": get(env, "AWS_SESSION_TOKEN", Value::Null)?,
            "region": get(env, "AWS_DEFAULT_REGION", "us-east-1")?,
        },
        "slack": {
            "notifications": {
                "bot_user_oauth_token": get(env, "SLACK_BOT_USER_OAUTH_TOKEN", Value::Null)?,
                "channel": get(env, "SLACK_BOT_USER_DEFAULT_CHANNEL", Value::Null)?,
            },
        },
    }))
}
