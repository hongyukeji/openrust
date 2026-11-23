use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;

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
    const CACHED: bool = false;

    fn get_facade_accessor() -> ServiceKey {
        "config".into()
    }
}

impl Config {
    /// Returns the value at a dot-separated key, if it exists.
    pub fn get(key: &str) -> Result<Option<Value>, BindingResolutionError> {
        Ok(Self::get_facade_root()?.get(key))
    }

    /// Returns the default only when the key is absent.
    pub fn get_or(key: &str, default: Value) -> Result<Value, BindingResolutionError> {
        Ok(Self::get(key)?.unwrap_or(default))
    }

    /// Reads multiple keys with per-key defaults, in input order.
    pub fn get_many<'a>(
        keys: impl IntoIterator<Item = (&'a str, Value)>,
    ) -> Result<IndexMap<String, Value>, BindingResolutionError> {
        let repository = Self::get_facade_root()?;
        Ok(keys
            .into_iter()
            .map(|(key, default)| (key.to_owned(), repository.get(key).unwrap_or(default)))
            .collect())
    }

    /// Deserializes a configuration value or section into a Rust type.
    pub fn deserialize<T: DeserializeOwned>(key: &str) -> Result<T, ConfigError> {
        typed(key, "compatible with the requested type", |value| {
            serde_json::from_value(value).ok()
        })
    }

    /// Deserializes a value, or uses the provided default when the key is absent.
    pub fn deserialize_or<T: DeserializeOwned>(key: &str, default: T) -> Result<T, ConfigError> {
        match Self::get(key)? {
            None => Ok(default),
            Some(value) => serde_json::from_value(value).map_err(|_| ConfigError::InvalidType {
                key: key.to_owned(),
                expected: "compatible with the requested type",
            }),
        }
    }

    pub fn has(key: &str) -> Result<bool, ConfigError> {
        Ok(Self::get_facade_root()?.has(key))
    }

    pub fn all() -> Result<BTreeMap<String, Value>, ConfigError> {
        Ok(Self::get_facade_root()?.all())
    }

    pub fn set(key: &str, value: Value) -> Result<(), ConfigError> {
        Self::get_facade_root()?.set(key, value)
    }

    pub fn prepend(key: &str, value: Value) -> Result<(), ConfigError> {
        Self::get_facade_root()?.prepend(key, value)
    }

    pub fn push(key: &str, value: Value) -> Result<(), ConfigError> {
        Self::get_facade_root()?.push(key, value)
    }

    pub fn string(key: &str) -> Result<String, ConfigError> {
        typed(key, "a string", |value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
    }

    /// Returns the configured string or the default if it cannot be read as a string.
    pub fn string_or(key: &str, default: impl Into<String>) -> String {
        Self::string(key).unwrap_or_else(|_| default.into())
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
            Value::Object(items) => Some(Collection::from_entries(items)),
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
