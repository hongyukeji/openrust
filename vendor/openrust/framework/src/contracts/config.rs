use std::collections::BTreeMap;

use crate::config::{ConfigError, Value};

/// Reads and updates application configuration.
pub trait Repository: Send + Sync {
    fn has(&self, key: &str) -> bool;
    fn get(&self, key: &str) -> Option<Value>;
    fn all(&self) -> BTreeMap<String, Value>;
    fn set(&self, key: &str, value: Value) -> Result<(), ConfigError>;
    fn prepend(&self, key: &str, value: Value) -> Result<(), ConfigError>;
    fn push(&self, key: &str, value: Value) -> Result<(), ConfigError>;
}
