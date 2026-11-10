use std::{collections::BTreeMap, sync::RwLock};

use super::{ConfigError, Value};
use crate::contracts::config;

/// Stores configuration values under dot-separated paths.
pub struct Repository {
    values: RwLock<BTreeMap<String, Value>>,
}

impl Repository {
    pub(crate) fn new(values: BTreeMap<String, Value>) -> Self {
        Self {
            values: RwLock::new(values),
        }
    }

    pub(crate) fn replace_with(&self, loaded: Self) {
        *self.values.write().expect("configuration lock poisoned") = loaded
            .values
            .into_inner()
            .expect("configuration lock poisoned");
    }

    fn insert_list_value(&self, key: &str, value: Value, prepend: bool) -> Result<(), ConfigError> {
        validate_key(key)?;
        let mut values = self.values.write().expect("configuration lock poisoned");
        let mut items = match get(&values, key).cloned() {
            Some(Value::Array(items)) => items,
            None => Vec::new(),
            Some(_) => {
                return Err(ConfigError::InvalidType {
                    key: key.to_owned(),
                    expected: "an array",
                });
            }
        };
        if prepend {
            items.insert(0, value);
        } else {
            items.push(value);
        }
        set(&mut values, key, Value::Array(items));
        Ok(())
    }
}

impl config::Repository for Repository {
    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    fn get(&self, key: &str) -> Option<Value> {
        get(
            &self.values.read().expect("configuration lock poisoned"),
            key,
        )
        .cloned()
    }

    fn all(&self) -> BTreeMap<String, Value> {
        self.values
            .read()
            .expect("configuration lock poisoned")
            .clone()
    }

    fn set(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        validate_key(key)?;
        set(
            &mut self.values.write().expect("configuration lock poisoned"),
            key,
            value,
        );
        Ok(())
    }

    fn prepend(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.insert_list_value(key, value, true)
    }

    fn push(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.insert_list_value(key, value, false)
    }
}

fn validate_key(key: &str) -> Result<(), ConfigError> {
    if key.split('.').any(str::is_empty) {
        return Err(ConfigError::InvalidKey {
            key: key.to_owned(),
        });
    }
    Ok(())
}

fn get<'a>(values: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a Value> {
    let mut path = key.split('.');
    let mut value = values.get(path.next()?)?;
    for segment in path {
        value = value.get(segment)?;
    }
    Some(value)
}

fn set(values: &mut BTreeMap<String, Value>, key: &str, value: Value) {
    let Some((root, path)) = key.split_once('.') else {
        values.insert(key.to_owned(), value);
        return;
    };
    let mut node = values.entry(root.to_owned()).or_insert(Value::Null);
    let mut path = path.split('.').peekable();
    while let Some(segment) = path.next() {
        if !node.is_object() {
            *node = Value::Object(Default::default());
        }
        let object = node
            .as_object_mut()
            .expect("configuration node is an object");
        if path.peek().is_none() {
            object.insert(segment.to_owned(), value);
            return;
        }
        node = object.entry(segment.to_owned()).or_insert(Value::Null);
    }
}
