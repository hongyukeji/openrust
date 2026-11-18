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
        | SMS_DRIVER 选择默认配置，默认使用 log 记录调试信息。正式发送时填写选中配置的凭据。
        | 派发时确定配置名，Worker 从自身环境读取凭据。
        |
        */
        "default": get(env, "SMS_DRIVER", "log")?,

        /*
        |--------------------------------------------------------------------------
        | 供应商和发送市场
        |--------------------------------------------------------------------------
        |
        | 外层键是配置名，driver 是注册的供应商驱动。同一驱动可对应多个具名配置。
        | 多账号或跨市场时，在此处添加配置并为其使用独立的环境变量。
        | 默认根据号码选择内地或国际接口，market 可限制发送市场。
        | 不同市场需要独立凭据、端点或发送身份时，在同一配置的 markets 下覆盖。
        |
        */
        "drivers": {
            "log": {"driver": "log"},
            "vonage": {"driver": "vonage"},
            "aliyun": {
                "driver": "aliyun",
                "market": get(env, "SMS_ALIYUN_MARKET", "auto")?,
                "endpoint": get(env, "SMS_ALIYUN_ENDPOINT", Value::Null)?,
                "access_key_id": get(env, "SMS_ALIYUN_ACCESS_KEY_ID", Value::Null)?,
                "access_key_secret": get(env, "SMS_ALIYUN_ACCESS_KEY_SECRET", Value::Null)?,
                "sign_name": get(env, "SMS_ALIYUN_SIGN_NAME", Value::Null)?,
                "sender_id": get(env, "SMS_ALIYUN_SENDER_ID", Value::Null)?,
                "message_type": get(env, "SMS_ALIYUN_MESSAGE_TYPE", Value::Null)?,
            },
            "smsbao": {
                "driver": "smsbao",
                "market": get(env, "SMS_SMSBAO_MARKET", "auto")?,
                "endpoint": get(env, "SMS_SMSBAO_ENDPOINT", Value::Null)?,
                "username": get(env, "SMS_SMSBAO_USERNAME", Value::Null)?,
                "api_key": get(env, "SMS_SMSBAO_API_KEY", Value::Null)?,
            },
            "yunpian": {
                "driver": "yunpian",
                "market": get(env, "SMS_YUNPIAN_MARKET", "auto")?,
                "endpoint": get(env, "SMS_YUNPIAN_ENDPOINT", Value::Null)?,
                "api_key": get(env, "SMS_YUNPIAN_API_KEY", Value::Null)?,
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
