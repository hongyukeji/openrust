use std::collections::BTreeMap;

use crate::{
    config::{ConfigError, Value},
    container::ServiceKey,
    contracts::{config::Repository, container::BindingResolutionError},
    support::Collection,
};

use super::Facade;

/// Reads the current application's configuration repository.
pub struct Config;

impl Facade for Config {
    type Service = dyn Repository;
    const cached: bool = false;

    fn getFacadeAccessor() -> ServiceKey {
        "config".into()
    }
}

impl Config {
    /// Returns the value at a dot-separated key, if it exists.
    pub fn get(key: &str) -> Result<Option<Value>, BindingResolutionError> {
        Ok(Self::getFacadeRoot()?.get(key))
    }
    pub fn has(key: &str) -> Result<bool, ConfigError> {
        Ok(Self::getFacadeRoot()?.has(key))
    }

    pub fn all() -> Result<BTreeMap<String, Value>, ConfigError> {
        Ok(Self::getFacadeRoot()?.all())
    }

    pub fn set(key: &str, value: Value) -> Result<(), ConfigError> {
        Self::getFacadeRoot()?.set(key, value)
    }

    pub fn prepend(key: &str, value: Value) -> Result<(), ConfigError> {
        Self::getFacadeRoot()?.prepend(key, value)
    }

    pub fn push(key: &str, value: Value) -> Result<(), ConfigError> {
        Self::getFacadeRoot()?.push(key, value)
    }

    pub fn string(key: &str) -> Result<String, ConfigError> {
        typed(key, "a string", |value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
    }

    pub fn integer(key: &str) -> Result<i64, ConfigError> {
        typed(key, "an integer", |value| value.as_i64())
    }

    pub fn float(key: &str) -> Result<f64, ConfigError> {
        typed(key, "a float", |value| match value {
            Value::Number(number) if number.is_f64() => number.as_f64(),
            _ => None,
        })
    }

    pub fn boolean(key: &str) -> Result<bool, ConfigError> {
        typed(key, "a boolean", |value| value.as_bool())
    }

    /// Accepts lists and named maps, corresponding to both forms of PHP arrays.
    pub fn array(key: &str) -> Result<Value, ConfigError> {
        typed(key, "an array", |value| match value {
            Value::Array(_) | Value::Object(_) => Some(value),
            _ => None,
        })
    }

    pub fn collection(key: &str) -> Result<Collection, ConfigError> {
        typed(key, "an array", |value| match value {
            Value::Array(items) => Some(items.into_iter().collect()),
            Value::Object(items) => Some(Collection::fromEntries(items)),
            _ => None,
        })
    }
}

fn typed<T>(
    key: &str,
    expected: &'static str,
    convert: impl FnOnce(Value) -> Option<T>,
) -> Result<T, ConfigError> {
    Config::get(key)?
        .and_then(convert)
        .ok_or_else(|| ConfigError::InvalidType {
            key: key.to_owned(),
            expected,
        })
}
