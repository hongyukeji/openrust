use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};

/*
 * 读取环境变量，并按默认值的类型解析。
 *
 * 变量未设置时直接返回默认值。默认值为布尔值或数字时，分别解析为布尔值或非负整数，
 * 格式无效时返回配置错误。默认值为 Null 时，null 或 (null) 才会解析为空值，
 * 两种写法均不区分大小写。其他取值按字符串保留，包括空字符串。
 */
pub(super) fn get(
    env: &Environment,
    key: &str,
    default: impl Into<Value>,
) -> Result<Value, ConfigError> {
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

pub(super) fn optional_bool(env: &Environment, key: &str) -> Result<Option<bool>, ConfigError> {
    env.get_utf8(key)?
        .filter(|value| !is_null(value))
        .map(|value| boolean(key, value))
        .transpose()
}

pub(super) fn optional_integer(env: &Environment, key: &str) -> Result<Option<u64>, ConfigError> {
    env.get_utf8(key)?
        .filter(|value| !is_null(value))
        .map(|value| {
            value
                .parse()
                .map_err(|_| invalid(key, "a non-negative integer"))
        })
        .transpose()
}

pub(super) fn list(value: &str) -> Vec<&str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect()
}

pub(super) fn slug(name: &str) -> String {
    openrust::support::Str::slug(name)
}

pub(super) fn path(value: &Path) -> Result<&str, ConfigError> {
    value
        .to_str()
        .ok_or_else(|| invalid("configuration path", "valid UTF-8"))
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
