use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | OpenID Connect 身份提供方
    |--------------------------------------------------------------------------
    |
    | 显式启用 oidc 编译特性后，work 使用 OIDC_ISSUER、OIDC_CLIENT_ID、
    | OIDC_CLIENT_SECRET 与 OIDC_REDIRECT_URI。默认只访问 issuer 同源 HTTPS
    | 端点；跨源需在 allowed_origins 明确列出。未配置时不会添加登录路由。
    | OIDC_POST_LOGOUT_REDIRECT_URI 为已在提供方登记的固定退出回调，不读取请求目标。
    | OIDC_REPLAY_STORE 缺省使用默认缓存；多副本必须选择共享原子缓存。
    |
    */
    Ok(json!({"providers":{"work":{
        "issuer":get(env,"OIDC_ISSUER",Value::Null)?,
        "client_id":get(env,"OIDC_CLIENT_ID",Value::Null)?,
        "client_secret":get(env,"OIDC_CLIENT_SECRET",Value::Null)?,
        "redirect":get(env,"OIDC_REDIRECT_URI",Value::Null)?,
        "post_logout_redirect":get(env,"OIDC_POST_LOGOUT_REDIRECT_URI",Value::Null)?,
        "replay_store":get(env,"OIDC_REPLAY_STORE",Value::Null)?,
        "scopes":["openid","profile","email"],
        "algorithms":["RS256"],
        "attempt_ttl":600,
        "max_token_age":600,
        "timeout":10,
        "leeway":0,
        "userinfo":true,
        "allow_loopback_http":false
    }}}))
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
