use serde_json::{Map, Value};

use super::data::{self, DataPathError};

/// Array and object helpers for ordered JSON values.
pub struct Arr;

#[allow(non_snake_case, reason = "Matches Laravel's Arr API")]
impl Arr {
    pub fn accessible(value: &Value) -> bool {
        value.is_array() || value.is_object()
    }

    pub fn exists(value: &Value, key: &str) -> bool {
        data::direct(value, key).is_some()
    }

    /// Reads an exact key before attempting a dot path.
    pub fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
        data::direct(value, key).or_else(|| {
            data::segments(key)
                .ok()?
                .into_iter()
                .try_fold(value, data::direct)
        })
    }

    pub fn has(value: &Value, key: &str) -> bool {
        Self::get(value, key).is_some()
    }

    pub fn hasAll(value: &Value, keys: &[&str]) -> bool {
        !keys.is_empty() && keys.iter().all(|key| Self::has(value, key))
    }

    pub fn hasAny(value: &Value, keys: &[&str]) -> bool {
        keys.iter().any(|key| Self::has(value, key))
    }

    pub fn isList(value: &Value) -> bool {
        match value {
            Value::Array(_) => true,
            Value::Object(values) => values
                .keys()
                .enumerate()
                .all(|(index, key)| key == &index.to_string()),
            _ => false,
        }
    }

    pub fn isAssoc(value: &Value) -> bool {
        Self::accessible(value) && !Self::isList(value)
    }

    /// Sets a dot path with literal path segments.
    pub fn set(value: &mut Value, key: &str, item: Value) -> Result<(), DataPathError> {
        data::set(value, key, item, true, false)
    }

    /// Flattens nested values. Empty arrays and objects remain leaves.
    pub fn dot(value: &Value) -> Result<Map<String, Value>, DataPathError> {
        data::check_depth(value, 0)?;
        let mut result = Map::new();
        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    flatten(&mut result, index.to_string(), item);
                }
            }
            Value::Object(items) => {
                for (key, item) in items {
                    flatten(&mut result, key.clone(), item);
                }
            }
            _ => {}
        }
        Ok(result)
    }

    pub fn undot(values: &Map<String, Value>) -> Result<Value, DataPathError> {
        let mut result = Value::Array(Vec::new());
        for (key, value) in values {
            let segments = data::segments(key)?;
            data::check_depth(value, 0)?;
            data::write(&mut result, &segments, value, true, false);
        }
        data::check_depth(&result, 0)?;
        Ok(result)
    }
}

fn flatten(result: &mut Map<String, Value>, prefix: String, value: &Value) {
    match value {
        Value::Array(items) if !items.is_empty() => {
            for (index, child) in items.iter().enumerate() {
                flatten(result, format!("{prefix}.{index}"), child);
            }
        }
        Value::Object(items) if !items.is_empty() => {
            for (key, child) in items {
                flatten(result, format!("{prefix}.{key}"), child);
            }
        }
        _ => {
            result.insert(prefix, value.clone());
        }
    }
}
