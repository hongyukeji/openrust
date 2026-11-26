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

/*
 * 读取环境变量，并按默认值的类型解析。
 *
 * 变量未设置时直接返回默认值。默认值为布尔值或数字时，分别解析为布尔值或非负整数，
 * 格式无效时返回配置错误。默认值为 Null 时，null 或 (null) 才会解析为空值，
 * 两种写法均不区分大小写。其他取值按字符串保留，包括空字符串。
 */
fn get(env: &Environment, key: &str, default: impl Into<Value>) -> Result<Value, ConfigError> {
    let default = default.into();
    let Some(value) = env.get_utf8(key)? else {
        return Ok(default);
    };
    match default {
        Value::Bool(_) => boolean(key, value).map(Value::from),
        Value::Number(_) => value
            .parse::<u64>()
            .map(Value::from)
            .map_err(|_| invalid(key, "a non-negative integer")),
        Value::Null if is_null(value) => Ok(Value::Null),
        _ => Ok(Value::from(value)),
    }
}

fn boolean(key: &str, value: &str) -> Result<bool, ConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "(true)" | "1" => Ok(true),
        "false" | "(false)" | "0" => Ok(false),
        _ => Err(invalid(key, "a boolean")),
    }
}

fn is_null(value: &str) -> bool {
    value.eq_ignore_ascii_case("null") || value.eq_ignore_ascii_case("(null)")
}

fn invalid(key: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.to_owned(),
        expected,
    }
}
