use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 启用 Reverb 与运行方式
        |--------------------------------------------------------------------------
        |
        | REVERB_ENABLED 控制是否启用，REVERB_MODE 选择运行模式。
        |
        */
        "enabled": get(env, "REVERB_ENABLED", false)?,
        "mode": get(env, "REVERB_MODE", "embedded")?,
        "default": get(env, "REVERB_SERVER", "reverb")?,

        /*
        |--------------------------------------------------------------------------
        | WebSocket 服务端
        |--------------------------------------------------------------------------
        |
        | REVERB_SERVER_HOST 和 REVERB_SERVER_PORT 设置监听地址。多实例可通过 Redis 同步广播。
        |
        */
        "servers": {
            "reverb": {
                "host": get(env, "REVERB_SERVER_HOST", "0.0.0.0")?,
                "port": get(env, "REVERB_SERVER_PORT", 8080)?,
                "path": get(env, "REVERB_SERVER_PATH", "")?,
                "hostname": get(env, "REVERB_HOST", Value::Null)?,
                "options": {"tls": {}},
                "max_request_size": get(env, "REVERB_MAX_REQUEST_SIZE", 10_000)?,
                "scaling": {
                    "enabled": get(env, "REVERB_SCALING_ENABLED", false)?,
                    "channel": get(env, "REVERB_SCALING_CHANNEL", "reverb")?,
                    "server": {
                        "url": get(env, "REDIS_URL", Value::Null)?,
                        "host": get(env, "REDIS_HOST", "127.0.0.1")?,
                        "port": get(env, "REDIS_PORT", 6379)?,
                        "username": get(env, "REDIS_USERNAME", Value::Null)?,
                        "password": get(env, "REDIS_PASSWORD", Value::Null)?,
                        "database": get(env, "REDIS_DB", 0)?,
                        "timeout": get(env, "REDIS_TIMEOUT", 60)?,
                    },
                },
                "pulse_ingest_interval": get(env, "REVERB_PULSE_INGEST_INTERVAL", 15)?,
                "telescope_ingest_interval": get(env, "REVERB_TELESCOPE_INGEST_INTERVAL", 15)?,
            },
        },

        /*
        |--------------------------------------------------------------------------
        | 应用凭据与连接限制
        |--------------------------------------------------------------------------
        |
        | 配置应用凭据、允许的来源和消息大小。
        | 对外发布地址使用 REVERB_HOST、REVERB_PORT 和 REVERB_SCHEME。
        |
        */
        "apps": {
            "provider": "config",
            "apps": [{
                "key": get(env, "REVERB_APP_KEY", Value::Null)?,
                "secret": get(env, "REVERB_APP_SECRET", Value::Null)?,
                "app_id": get(env, "REVERB_APP_ID", Value::Null)?,
                "options": publish_options(env)?,
                "allowed_origins": ["*"],
                "ping_interval": get(env, "REVERB_APP_PING_INTERVAL", 60)?,
                "activity_timeout": get(env, "REVERB_APP_ACTIVITY_TIMEOUT", 30)?,
                "max_connections": optional_integer(env, "REVERB_APP_MAX_CONNECTIONS")?,
                "max_message_size": get(env, "REVERB_APP_MAX_MESSAGE_SIZE", 10_000)?,
                "accept_client_events_from": get(env, "REVERB_APP_ACCEPT_CLIENT_EVENTS_FROM", "members")?,
            }],
        },
    }))
}

pub(super) fn publish_options(env: &Environment) -> Result<Value, ConfigError> {
    let scheme = get(env, "REVERB_SCHEME", "https")?;
    Ok(json!({
        "host": get(env, "REVERB_HOST", Value::Null)?,
        "port": get(env, "REVERB_PORT", 443)?,
        "path": get(env, "REVERB_SERVER_PATH", "")?,
        "scheme": scheme,
        "useTLS": scheme == "https",
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

fn optional_integer(env: &Environment, key: &str) -> Result<Option<u64>, ConfigError> {
    env.get_utf8(key)?
        .filter(|value| !is_null(value))
        .map(|value| {
            value
                .parse()
                .map_err(|_| invalid(key, "a non-negative integer"))
        })
        .transpose()
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
