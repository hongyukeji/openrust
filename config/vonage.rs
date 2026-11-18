use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | Vonage 短信凭据
    |--------------------------------------------------------------------------
    |
    | 通过环境变量配置 Vonage 短信的发件人、API 密钥和密钥密码。
    |
    */
    Ok(json!({
        "sms_from": get(env, "VONAGE_SMS_FROM", Value::Null)?,
        "api_key": get(env, "VONAGE_KEY", Value::Null)?,
        "api_secret": get(env, "VONAGE_SECRET", Value::Null)?,
    }))
}
