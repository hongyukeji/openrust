use serde_json::{Map, Value};

use super::data::{self, DataPathError};

mod operations;

pub(super) use operations::random_index;
pub(crate) use operations::truthy;

/// A missing array path or a value of the wrong type.
#[derive(Debug, PartialEq, Eq)]
pub enum ArrValueError {
    Missing,
    Expected(&'static str),
    Multiple(usize),
    SampleSize,
    RandomUnavailable,
    KeyOverflow,
    Path(DataPathError),
}

impl std::fmt::Display for ArrValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Multiple(count) => write!(f, "expected one array item, found {count}"),
            Self::RandomUnavailable => f.write_str("system randomness is unavailable"),
            Self::SampleSize => f.write_str("sample size exceeds array length"),
            Self::KeyOverflow => f.write_str("next integer key is out of range"),
            Self::Path(error) => error.fmt(f),
            Self::Missing => f.write_str("array value is missing"),
            Self::Expected(kind) => write!(f, "array value must be {kind}"),
        }
    }
}

impl std::error::Error for ArrValueError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Path(source) => Some(source),
            _ => None,
        }
    }
}

impl From<DataPathError> for ArrValueError {
    fn from(error: DataPathError) -> Self {
        Self::Path(error)
    }
}

/// Array and object helpers for ordered JSON values.
pub struct Arr;

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

    pub fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Value, ArrValueError> {
        let value = Self::get(value, key).ok_or(ArrValueError::Missing)?;
        if Self::accessible(value) {
            Ok(value)
        } else {
            Err(ArrValueError::Expected("an array or object"))
        }
    }

    pub fn boolean(value: &Value, key: &str) -> Result<bool, ArrValueError> {
        Self::get(value, key)
            .ok_or(ArrValueError::Missing)?
            .as_bool()
            .ok_or(ArrValueError::Expected("a boolean"))
    }

    pub fn integer(value: &Value, key: &str) -> Result<i64, ArrValueError> {
        Self::get(value, key)
            .ok_or(ArrValueError::Missing)?
            .as_i64()
            .ok_or(ArrValueError::Expected("a signed integer"))
    }

    pub fn float(value: &Value, key: &str) -> Result<f64, ArrValueError> {
        let value = Self::get(value, key).ok_or(ArrValueError::Missing)?;
        value
            .as_number()
            .filter(|number| number.is_f64())
            .and_then(serde_json::Number::as_f64)
            .ok_or(ArrValueError::Expected("a float"))
    }

    pub fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, ArrValueError> {
        Self::get(value, key)
            .ok_or(ArrValueError::Missing)?
            .as_str()
            .ok_or(ArrValueError::Expected("a string"))
    }

    pub fn has(value: &Value, key: &str) -> bool {
        Self::get(value, key).is_some()
    }

    pub fn has_all(value: &Value, keys: &[&str]) -> bool {
        !keys.is_empty() && keys.iter().all(|key| Self::has(value, key))
    }

    pub fn has_any(value: &Value, keys: &[&str]) -> bool {
        keys.iter().any(|key| Self::has(value, key))
    }

    pub fn is_list(value: &Value) -> bool {
        match value {
            Value::Array(_) => true,
            Value::Object(values) => values
                .keys()
                .enumerate()
                .all(|(index, key)| key == &index.to_string()),
            _ => false,
        }
    }

    pub fn is_assoc(value: &Value) -> bool {
        Self::accessible(value) && !Self::is_list(value)
    }

    /// Sets a dot path with literal path segments.
    pub fn set(value: &mut Value, key: &str, item: Value) -> Result<(), DataPathError> {
        data::set(value, key, item, true, false)
    }

    pub(crate) fn with_paths(
        mut value: Value,
        items: impl IntoIterator<Item = (String, Value)>,
    ) -> Result<Value, DataPathError> {
        data::check_depth(&value, 0)?;
        for (key, item) in items {
            let path = data::segments(&key)?;
            data::check_depth(&item, 0)?;
            data::write(&mut value, &path, &item, true, false);
        }
        data::check_depth(&value, 0)?;
        Ok(value)
    }

    /// Adds a value at a missing or null path.
    pub fn add(value: &mut Value, key: &str, item: Value) -> Result<(), DataPathError> {
        if Self::get(value, key).is_none_or(Value::is_null) {
            Self::set(value, key, item)?;
        }
        Ok(())
    }

    pub fn forget(value: &mut Value, key: &str) -> Result<(), DataPathError> {
        if let Some(object) = value.as_object_mut()
            && object.shift_remove(key).is_some()
        {
            return Ok(());
        }
        data::forget(value, &data::segments(key)?, false);
        Ok(())
    }

    pub fn pull(value: &mut Value, key: &str) -> Result<Option<Value>, DataPathError> {
        data::segments(key)?;
        let found = Self::get(value, key).cloned();
        Self::forget(value, key)?;
        Ok(found)
    }

    pub fn wrap(value: Value) -> Value {
        match value {
            Value::Null => Value::Array(Vec::new()),
            Value::Array(_) | Value::Object(_) => value,
            value => Value::Array(vec![value]),
        }
    }

    /// Builds an RFC 3986 query string from ordered JSON array entries.
    pub fn query(value: &Value) -> crate::routing::UrlResult<String> {
        data::check_depth(value, 0).map_err(|_| crate::routing::UrlError::InvalidParameter)?;
        crate::routing::encode_query_value(value.clone())
    }

    /// Flattens nested values. Empty arrays and objects remain leaves.
    pub fn dot(value: &Value) -> Result<Map<String, Value>, DataPathError> {
        Self::dot_with_depth(value, "", None)
    }

    pub fn dot_with_depth(
        value: &Value,
        prefix: &str,
        depth: Option<usize>,
    ) -> Result<Map<String, Value>, DataPathError> {
        data::check_depth(value, 0)?;
        let mut result = Map::new();
        let depth = depth.unwrap_or(usize::MAX);
        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    flatten(&mut result, format!("{prefix}{index}"), item, depth);
                }
            }
            Value::Object(items) => {
                for (key, item) in items {
                    flatten(&mut result, format!("{prefix}{key}"), item, depth);
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

fn flatten(result: &mut Map<String, Value>, prefix: String, value: &Value, depth: usize) {
    match value {
        Value::Array(items) if depth > 0 && !items.is_empty() => {
            for (index, child) in items.iter().enumerate() {
                flatten(result, format!("{prefix}.{index}"), child, depth - 1);
            }
        }
        Value::Object(items) if depth > 0 && !items.is_empty() => {
            for (key, child) in items {
                flatten(result, format!("{prefix}.{key}"), child, depth - 1);
            }
        }
        _ => {
            result.insert(prefix, value.clone());
        }
    }
}
