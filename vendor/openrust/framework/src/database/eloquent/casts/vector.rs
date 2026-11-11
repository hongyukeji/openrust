use serde_json::{Number, Value as Json};

use crate::database::{DatabaseError, DatabaseResult, Value};

use super::AttributeCast;

/// Converts numeric arrays to vector attributes.
pub struct AsVector;

impl AsVector {
    pub const fn cast() -> AttributeCast {
        AttributeCast::Vector
    }
}

pub(super) fn read(value: &Value) -> DatabaseResult<Value> {
    let numbers = match value {
        Value::Null => return Ok(Value::Null),
        Value::Text(text) => {
            let parsed = serde_json::from_str(text).map_err(|_| invalid())?;
            normalize(&parsed)?
        }
        Value::Json(value) => normalize(value)?,
        Value::Bytes(bytes) => {
            let (components, remainder) = bytes.as_chunks::<4>();
            if !remainder.is_empty() {
                return Err(invalid());
            }
            components
                .iter()
                .map(|bytes| number(f64::from(f32::from_le_bytes(*bytes))))
                .collect::<DatabaseResult<_>>()?
        }
        _ => return Err(invalid()),
    };
    Ok(Value::Json(Json::Array(numbers)))
}

pub(super) fn write(value: Value) -> DatabaseResult<Value> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::Json(value) => serde_json::to_string(&normalize(&value)?)
            .map(Value::Text)
            .map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

fn normalize(value: &Json) -> DatabaseResult<Vec<Json>> {
    match value {
        Json::Array(values) => values.iter().map(component).collect(),
        Json::Object(values) => values.values().map(component).collect(),
        _ => Err(invalid()),
    }
}

fn component(value: &Json) -> DatabaseResult<Json> {
    let value = match value {
        Json::Number(value) => value.as_f64().ok_or_else(invalid)?,
        Json::String(value) => super::primitive::numeric_prefix(value)
            .map_or(Ok(0.0), |prefix| prefix.parse().map_err(|_| invalid()))?,
        Json::Bool(value) => f64::from(u8::from(*value)),
        Json::Null => 0.0,
        Json::Array(values) => f64::from(u8::from(!values.is_empty())),
        Json::Object(values) => f64::from(u8::from(!values.is_empty())),
    };
    number(value)
}

fn number(value: f64) -> DatabaseResult<Json> {
    Number::from_f64(value)
        .map(Json::Number)
        .ok_or_else(invalid)
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("vector attribute")
}
