use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "postmark": {"token": get(env, "POSTMARK_TOKEN", Value::Null)?},
        "resend": {"key": get(env, "RESEND_KEY", Value::Null)?},
        "ses": {
            "key": get(env, "AWS_ACCESS_KEY_ID", Value::Null)?,
            "secret": get(env, "AWS_SECRET_ACCESS_KEY", Value::Null)?,
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
