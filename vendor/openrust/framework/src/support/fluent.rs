use std::fmt;

use serde::Serialize;
use serde_json::Value;

use super::data;

/// Ordered attributes with Laravel-style dot-path access.
#[derive(Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Fluent(Value);

impl Default for Fluent {
    fn default() -> Self {
        Self(Value::Array(Vec::new()))
    }
}

impl fmt::Debug for Fluent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Fluent").finish_non_exhaustive()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Fluent API")]
impl Fluent {
    /// Constructs an attribute bag from JSON array or object data.
    pub fn make(attributes: Value) -> Result<Self, FluentError> {
        if !matches!(attributes, Value::Array(_) | Value::Object(_)) {
            return Err(FluentError::Shape);
        }
        data::check_depth(&attributes, 0)?;
        Ok(Self(data::normalize(attributes)))
    }

    /// Reads a dot path with wildcard, first and last selectors.
    pub fn get(&self, key: &str) -> Option<Value> {
        data::read(&self.0, &data::segments(key).ok()?)
    }

    /// Reads an exact key, including keys that contain dots.
    pub fn value(&self, key: &str) -> Option<&Value> {
        data::direct(&self.0, key)
    }

    /// Checks a key or path. Explicit null counts as present.
    pub fn has(&self, key: &str) -> bool {
        self.value(key).is_some() || key.split('.').try_fold(&self.0, data::direct).is_some()
    }

    /// Updates a dot path. A wildcard writes to each existing child.
    pub fn set(&mut self, key: &str, value: Value) -> Result<&mut Self, FluentError> {
        let parts = data::segments(key)?;
        data::check_depth(&value, 0)?;
        let mut attributes = self.0.clone();
        data::write(&mut attributes, &parts, &data::normalize(value), true, true);
        data::check_depth(&attributes, 0)?;
        self.0 = attributes;
        Ok(self)
    }

    /// Creates an independent bag from the selected attribute.
    pub fn scope(&self, key: &str) -> Self {
        match self.get(key) {
            None | Some(Value::Null) => Self::default(),
            Some(value @ (Value::Array(_) | Value::Object(_))) => Self(value),
            Some(value) => Self(Value::Array(vec![value])),
        }
    }

    pub fn all(&self) -> &Value {
        &self.0
    }

    pub fn toArray(&self) -> Value {
        self.0.clone()
    }

    pub fn toJson(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn isEmpty(&self) -> bool {
        match &self.0 {
            Value::Array(items) => items.is_empty(),
            Value::Object(items) => items.is_empty(),
            _ => unreachable!("Fluent holds array or object attributes"),
        }
    }

    pub fn isNotEmpty(&self) -> bool {
        !self.isEmpty()
    }
}

/// An invalid Fluent attribute bag or path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FluentError {
    Shape,
    Depth,
}

impl fmt::Display for FluentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Shape => "Fluent attributes must be an array or object",
            Self::Depth => "Fluent attributes or path exceed maximum depth",
        })
    }
}

impl std::error::Error for FluentError {}

impl From<data::DataPathError> for FluentError {
    fn from(_: data::DataPathError) -> Self {
        Self::Depth
    }
}
