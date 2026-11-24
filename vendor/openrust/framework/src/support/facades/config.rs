use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;

use crate::{
    config::{ConfigError, ConfigKey, Value},
    container::ServiceKey,
    contracts::{config::Repository, container::BindingResolutionError},
    support::Collection,
};

use super::Facade;

macro_rules! facade_reader {
    ($name:ident,$with:ident,$lazy:ident,$output:ty,$default:ty,$arg:ty,$make:path) => {
        pub fn $name(key: &str) -> Result<$output, ConfigError> {
            Self::get_facade_root()?.$name(key)
        }
        pub fn $with(key: &str, default: $arg) -> Result<$output, ConfigError> {
            Self::get_facade_root()?.$with(key, $make(default))
        }
        pub fn $lazy(
            key: &str,
            default: impl FnOnce() -> $default,
        ) -> Result<$output, ConfigError> {
            Self::get_facade_root()?.$lazy(key, Box::new(default))
        }
    };
}

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
    pub fn r#macro<A: std::any::Any, T: std::any::Any>(
        name: impl Into<String>,
        callback: impl Fn(&dyn Repository, A) -> Result<T, ConfigError> + Send + Sync + 'static,
    ) -> Result<(), ConfigError> {
        Self::get_facade_root()?
            .register_macro(&name.into(), crate::config::ConfigMacro::new(callback))
    }
    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, crate::config::ConfigMacro)>,
        replace: bool,
    ) -> Result<(), ConfigError> {
        let repository = Self::get_facade_root()?;
        for (name, callback) in macros {
            let name = name.into();
            if replace || !repository.has_macro(&name)? {
                repository.register_macro(&name, callback)?;
            }
        }
        Ok(())
    }
    pub fn mixin_using<
        K: Into<String>,
        F: FnOnce() -> Result<crate::config::ConfigMacro, ConfigError>,
    >(
        factories: impl IntoIterator<Item = (K, F)>,
        replace: bool,
    ) -> Result<(), ConfigError> {
        let repository = Self::get_facade_root()?;
        for (name, factory) in factories {
            let name = name.into();
            if replace || !repository.has_macro(&name)? {
                repository.register_macro(&name, factory()?)?;
            }
        }
        Ok(())
    }
    pub fn has_macro(name: &str) -> Result<bool, ConfigError> {
        Self::get_facade_root()?.has_macro(name)
    }
    pub fn flush_macros() -> Result<(), ConfigError> {
        Self::get_facade_root()?.flush_macros()
    }
    pub fn macro_call<T: std::any::Any, A: std::any::Any>(
        name: &str,
        arguments: A,
    ) -> Result<T, ConfigError> {
        let repository = Self::get_facade_root()?;
        repository
            .invoke_macro(
                repository.as_ref(),
                name,
                Box::new(arguments),
                std::any::type_name::<A>(),
            )?
            .into_value(name)
    }

    /// Returns the value at a dot-separated key, if it exists.
    pub fn get(key: &str) -> Result<Option<Value>, BindingResolutionError> {
        Ok(Self::get_facade_root()?.get(key))
    }

    pub fn offset_exists(key: &str) -> Result<bool, ConfigError> {
        Ok(Self::get_facade_root()?.offset_exists(key))
    }
    pub fn offset_get(key: &str) -> Result<Option<Value>, ConfigError> {
        Ok(Self::get_facade_root()?.offset_get(key))
    }
    pub fn offset_set(key: &str, value: Value) -> Result<(), ConfigError> {
        Self::get_facade_root()?.offset_set(key, value)
    }
    pub fn offset_unset(key: &str) -> Result<(), ConfigError> {
        Self::get_facade_root()?.offset_unset(key)
    }

    /// Returns the default only when the key is absent.
    pub fn get_or(key: &str, default: Value) -> Result<Value, BindingResolutionError> {
        Ok(Self::get_facade_root()?.get_or(key, default))
    }

    /// 按输入顺序读取普通键、逐键默认值或惰性默认值。
    pub fn get_many<'a, K: Into<ConfigKey<'a>>>(
        keys: impl IntoIterator<Item = K>,
    ) -> Result<IndexMap<String, Value>, BindingResolutionError> {
        Ok(Self::get_facade_root()?.get_many(keys.into_iter().map(Into::into).collect()))
    }
    pub fn get_or_else(
        key: &str,
        default: impl FnOnce() -> Value,
    ) -> Result<Value, BindingResolutionError> {
        Ok(Self::get_facade_root()?.get_or_else(key, Box::new(default)))
    }
    pub fn set_many<K: Into<String>>(
        values: impl IntoIterator<Item = (K, Value)>,
    ) -> Result<(), ConfigError> {
        Self::get_facade_root()?.set_many(
            &values
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect::<Vec<_>>(),
        )
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

    // 兼容既有欢迎页的容错扩展；严格缺键默认值使用string_with_default。
    pub fn string_or(key: &str, default: impl Into<String>) -> String {
        Self::string(key).unwrap_or_else(|_| default.into())
    }
    facade_reader!(
        string,
        string_with_default,
        string_or_else,
        String,
        String,
        impl Into<String>,
        Into::into
    );
    facade_reader!(
        integer,
        integer_with_default,
        integer_or_else,
        i64,
        i64,
        i64,
        std::convert::identity
    );
    facade_reader!(
        float,
        float_with_default,
        float_or_else,
        f64,
        f64,
        f64,
        std::convert::identity
    );
    facade_reader!(
        boolean,
        boolean_with_default,
        boolean_or_else,
        bool,
        bool,
        bool,
        std::convert::identity
    );
    facade_reader!(
        array,
        array_with_default,
        array_or_else,
        Value,
        Value,
        Value,
        std::convert::identity
    );
    facade_reader!(
        collection,
        collection_with_default,
        collection_or_else,
        Collection,
        Value,
        Value,
        std::convert::identity
    );
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
