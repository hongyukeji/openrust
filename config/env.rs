use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};

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
    name.split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

pub(super) fn snake(name: &str) -> String {
    let mut result = String::new();
    for character in name.chars().filter(|character| !character.is_whitespace()) {
        if character.is_uppercase() && !result.is_empty() {
            result.push('_');
        }
        result.extend(character.to_lowercase());
    }
    result
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
