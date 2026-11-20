use super::env::get;
use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;
pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 支付商户与默认配置
    |--------------------------------------------------------------------------
    |
    | 通过 pay Cargo feature 启用。PAY_DRIVER 选择具名商户；不同商户可增加配置名。
    | 私钥与平台公钥使用受部署权限保护的 PEM 文件，不把密钥加入版本控制。
    | 微信首期使用 API v3 公钥模式，支付宝使用 RSA2 普通公钥模式。
    |
    */
    Ok(json!({
        "default": get(env, "PAY_DRIVER", "wechat")?,
        "drivers": {
            "wechat": {
                "driver": "wechat",
                "app_id": get(env, "PAY_WECHAT_APP_ID", "")?,
                "mch_id": get(env, "PAY_WECHAT_MCH_ID", "")?,
                "serial_no": get(env, "PAY_WECHAT_SERIAL_NO", "")?,
                "private_key_path": get(env, "PAY_WECHAT_PRIVATE_KEY_PATH", "")?,
                "api_v3_key": get(env, "PAY_WECHAT_API_V3_KEY", "")?,
                "public_key_id": get(env, "PAY_WECHAT_PUBLIC_KEY_ID", "")?,
                "public_key_path": get(env, "PAY_WECHAT_PUBLIC_KEY_PATH", "")?,
            },
            "alipay": {
                "driver": "alipay",
                "app_id": get(env, "PAY_ALIPAY_APP_ID", "")?,
                "seller_id": get(env, "PAY_ALIPAY_SELLER_ID", "")?,
                "private_key_path": get(env, "PAY_ALIPAY_PRIVATE_KEY_PATH", "")?,
                "public_key_path": get(env, "PAY_ALIPAY_PUBLIC_KEY_PATH", "")?,
            },
        },
    }))
}
