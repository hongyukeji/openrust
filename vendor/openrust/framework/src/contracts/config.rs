use std::collections::BTreeMap;

use crate::config::{ConfigError, ConfigKey, Value};
use crate::support::Collection;

macro_rules! typed_reader {
    ($name:ident,$with:ident,$lazy:ident,$output:ty,$default:ty,$convert:ident,$fallback:expr,$expected:literal) => {
        fn $name(&self, key: &str) -> Result<$output, ConfigError> {
            crate::config::typed::read(
                self,
                key,
                $expected,
                || Err(crate::config::typed::invalid(key, $expected)),
                crate::config::typed::$convert,
            )
        }
        fn $with(&self, key: &str, default: $default) -> Result<$output, ConfigError> {
            crate::config::typed::read(
                self,
                key,
                $expected,
                || {
                    ($fallback)(default)
                        .ok_or_else(|| crate::config::typed::invalid(key, $expected))
                },
                crate::config::typed::$convert,
            )
        }
        fn $lazy(
            &self,
            key: &str,
            default: Box<dyn FnOnce() -> $default + '_>,
        ) -> Result<$output, ConfigError> {
            crate::config::typed::read(
                self,
                key,
                $expected,
                || {
                    ($fallback)(default())
                        .ok_or_else(|| crate::config::typed::invalid(key, $expected))
                },
                crate::config::typed::$convert,
            )
        }
    };
}
/// Reads and updates application configuration.
pub trait Repository: Send + Sync {
    fn has(&self, key: &str) -> bool;
    fn get(&self, key: &str) -> Option<Value>;
    fn all(&self) -> BTreeMap<String, Value>;
    fn set(&self, key: &str, value: Value) -> Result<(), ConfigError>;
    fn prepend(&self, key: &str, value: Value) -> Result<(), ConfigError>;
    fn push(&self, key: &str, value: Value) -> Result<(), ConfigError>;
    fn offset_exists(&self, key: &str) -> bool {
        self.has(key)
    }
    fn offset_get(&self, key: &str) -> Option<Value> {
        self.get(key)
    }
    fn offset_set(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.set(key, value)
    }
    fn offset_unset(&self, key: &str) -> Result<(), ConfigError> {
        self.set(key, Value::Null)
    }
    fn get_or(&self, key: &str, default: Value) -> Value {
        self.get(key).unwrap_or(default)
    }
    fn get_or_else(&self, key: &str, default: Box<dyn FnOnce() -> Value + '_>) -> Value {
        self.get(key).unwrap_or_else(default)
    }
    fn get_many(&self, keys: Vec<ConfigKey<'_>>) -> indexmap::IndexMap<String, Value> {
        keys.into_iter()
            .map(|ConfigKey { name, default }| {
                let value = self.get(&name).unwrap_or_else(|| default.resolve());
                (name, value)
            })
            .collect()
    }
    fn set_many(&self, values: &[(String, Value)]) -> Result<(), ConfigError> {
        for (key, value) in values {
            self.set(key, value.clone())?;
        }
        Ok(())
    }
    typed_reader!(
        string,
        string_with_default,
        string_or_else,
        String,
        String,
        string,
        Some,
        "a string"
    );
    typed_reader!(
        integer,
        integer_with_default,
        integer_or_else,
        i64,
        i64,
        integer,
        Some,
        "an integer"
    );
    typed_reader!(
        float,
        float_with_default,
        float_or_else,
        f64,
        f64,
        float,
        Some,
        "a float"
    );
    typed_reader!(
        boolean,
        boolean_with_default,
        boolean_or_else,
        bool,
        bool,
        boolean,
        Some,
        "a boolean"
    );
    typed_reader!(
        array,
        array_with_default,
        array_or_else,
        Value,
        Value,
        array,
        crate::config::typed::array,
        "an array"
    );
    typed_reader!(
        collection,
        collection_with_default,
        collection_or_else,
        Collection,
        Value,
        collection,
        crate::config::typed::collection,
        "an array"
    );
}
