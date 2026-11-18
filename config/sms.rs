use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 默认短信配置
        |--------------------------------------------------------------------------
        |
        | SMS_DRIVER 选择具名配置，默认沿用 Vonage。
        | 派发时确定配置名，Worker 从自身环境读取凭据。
        |
        */
        "default": get(env, "SMS_DRIVER", "vonage")?,

        /*
        |--------------------------------------------------------------------------
        | 供应商和发送市场
        |--------------------------------------------------------------------------
        |
        | 阿里云内地模板和国际／港澳台文本使用独立配置。腾讯云和 Twilio 默认接受带区号的号码；
        | 可将相应的 SMS_*_MARKET 设为 mainland 或 international。
        |
        */
        "drivers": {
            "vonage": {"driver": "vonage"},
            "aliyun": {
                "driver": "aliyun",
                "endpoint": get(env, "SMS_ALIYUN_ENDPOINT", Value::Null)?,
                "access_key_id": get(env, "SMS_ALIYUN_ACCESS_KEY_ID", Value::Null)?,
                "access_key_secret": get(env, "SMS_ALIYUN_ACCESS_KEY_SECRET", Value::Null)?,
                "sign_name": get(env, "SMS_ALIYUN_SIGN_NAME", Value::Null)?,
            },
            "aliyun_international": {
                "driver": "aliyun_international",
                "endpoint": get(env, "SMS_ALIYUN_INTERNATIONAL_ENDPOINT", Value::Null)?,
                "access_key_id": get(env, "SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_ID", Value::Null)?,
                "access_key_secret": get(env, "SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_SECRET", Value::Null)?,
                "sender_id": get(env, "SMS_ALIYUN_INTERNATIONAL_SENDER_ID", Value::Null)?,
                "message_type": get(env, "SMS_ALIYUN_INTERNATIONAL_MESSAGE_TYPE", Value::Null)?,
            },
            "tencent": {
                "driver": "tencent",
                "market": get(env, "SMS_TENCENT_MARKET", Value::Null)?,
                "secret_id": get(env, "SMS_TENCENT_SECRET_ID", Value::Null)?,
                "secret_key": get(env, "SMS_TENCENT_SECRET_KEY", Value::Null)?,
                "sdk_app_id": get(env, "SMS_TENCENT_SDK_APP_ID", Value::Null)?,
                "sign_name": get(env, "SMS_TENCENT_SIGN_NAME", Value::Null)?,
                "region": get(env, "SMS_TENCENT_REGION", "ap-guangzhou")?,
            },
            "twilio": {
                "driver": "twilio",
                "market": get(env, "SMS_TWILIO_MARKET", Value::Null)?,
                "account_sid": get(env, "SMS_TWILIO_ACCOUNT_SID", Value::Null)?,
                "api_key_sid": get(env, "SMS_TWILIO_API_KEY_SID", Value::Null)?,
                "api_key_secret": get(env, "SMS_TWILIO_API_KEY_SECRET", Value::Null)?,
                "from": get(env, "SMS_TWILIO_FROM", Value::Null)?,
                "messaging_service_sid": get(env, "SMS_TWILIO_MESSAGING_SERVICE_SID", Value::Null)?,
            },
        },
    }))
}
