use crate::database::{DatabaseError, DatabaseResult, Value};

use super::{
    BinaryFormat, CastsAttributes, CastsInboundAttributes, ClassCast, EncryptedCast, EnumCast,
    EnumCollectionCast, JsonCollectionCast, ValueObjectCast,
};

/// A declared conversion of an Eloquent attribute.
#[derive(Clone, Copy)]
pub enum AttributeCast {
    Boolean,
    Integer,
    Float,
    String,
    Json,
    Hashed,
    Binary(BinaryFormat),
    Vector,
    JsonCollection(JsonCollectionCast),
    Decimal(u32),
    Date,
    DateTime,
    DateWithFormat(&'static str),
    DateTimeWithFormat(&'static str),
    Timestamp,
    Enum(EnumCast),
    EnumCollection(EnumCollectionCast),
    Object(ValueObjectCast),
    Encrypted(EncryptedCast),
    Custom(&'static dyn CastsAttributes),
    Class(ClassCast),
    Inbound(&'static dyn CastsInboundAttributes),
}

impl AttributeCast {
    pub(super) fn class_caster(self) -> Option<&'static dyn CastsInboundAttributes> {
        match self {
            Self::Custom(caster) => Some(caster),
            Self::Inbound(caster) => Some(caster),
            _ => None,
        }
    }

    pub(super) fn read(self, value: &Value) -> DatabaseResult<Value> {
        if matches!(value, Value::Null) {
            return Ok(Value::Null);
        }
        let invalid = || DatabaseError::InvalidQuery("attribute cast");
        match (self, value) {
            (Self::Hashed, value) => Ok(value.clone()),
            (Self::Enum(cast), value) => cast.normalize(value),
            (Self::Binary(format), value) => format.read(value),
            (Self::Vector, value) => super::vector::read(value),
            (Self::EnumCollection(cast), value) => cast.read(value),
            (Self::JsonCollection(cast), value) => cast.read(value),
            (Self::Boolean, Value::Bool(_))
            | (Self::Integer, Value::Integer(_))
            | (Self::String, Value::Text(_))
            | (Self::Json, Value::Json(_)) => Ok(value.clone()),
            (Self::Boolean, Value::Integer(n)) => Ok(Value::Bool(*n != 0)),
            (Self::Boolean, Value::Unsigned(n)) => Ok(Value::Bool(*n != 0)),
            (Self::Boolean, Value::Real(n)) => Ok(Value::Bool(*n != 0.0)),
            (Self::Boolean, Value::Text(text)) => Ok(Value::Bool(!text.is_empty() && text != "0")),
            (Self::Integer, Value::Unsigned(n)) => {
                i64::try_from(*n).map(Value::Integer).map_err(|_| invalid())
            }
            (Self::Integer, Value::Bool(flag)) => Ok(Value::Integer(i64::from(*flag))),
            (Self::Integer, Value::Text(text)) => Ok(Value::Integer(integer_string(text))),
            (Self::Integer, Value::Real(n)) if (-TWO_TO_63..TWO_TO_63).contains(n) => {
                Ok(Value::Integer(truncate(*n)))
            }
            (Self::Float, Value::Bool(flag)) => Ok(Value::Real(f64::from(u8::from(*flag)))),
            (Self::Float, Value::Integer(_) | Value::Unsigned(_)) => {
                let number = scalar_string(value)?.parse().map_err(|_| invalid())?;
                Ok(Value::Real(number))
            }
            (Self::Float, Value::Real(n)) if n.is_finite() => Ok(Value::Real(*n)),
            (Self::Float, Value::Text(text)) => {
                let number = numeric_prefix(text)
                    .and_then(|prefix| prefix.parse::<f64>().ok())
                    .unwrap_or(0.0);
                if !number.is_finite() || matches!(text.as_str(), "Infinity" | "-Infinity" | "NaN")
                {
                    return Err(invalid());
                }
                Ok(Value::Real(number))
            }
            (Self::String, _) => Ok(Value::Text(scalar_string(value)?)),
            (Self::Decimal(scale), _) => {
                super::decimal::read(&scalar_string(value)?, scale).map(Value::Text)
            }
            (Self::Json, Value::Text(text)) => {
                Ok(serde_json::from_str(text).map_or(Value::Null, Value::Json))
            }
            _ => Err(invalid()),
        }
    }

    pub(super) fn equivalent(self, current: &Value, original: &Value) -> DatabaseResult<bool> {
        match (self.read(current)?, self.read(original)?) {
            (Value::Real(current), Value::Real(original)) if matches!(self, Self::Float) => {
                Ok((current - original).abs() < f64::EPSILON * 4.0)
            }
            (current, original) => Ok(current == original),
        }
    }

    pub(super) fn write(self, value: Value) -> DatabaseResult<Value> {
        if let Self::Binary(format) = self {
            return format.write(value);
        }
        if matches!(self, Self::Vector) {
            return super::vector::write(value);
        }
        if let Self::JsonCollection(cast) = self {
            return cast.write(value);
        }
        if let Self::Enum(cast) = self {
            return cast.normalize(&value);
        }
        if let Self::EnumCollection(cast) = self {
            return cast.write(value);
        }
        if matches!(self, Self::Json) && !matches!(value, Value::Null | Value::Json(_)) {
            return json_value(value).map(Value::Json);
        }
        Ok(value)
    }
}

pub(in crate::database::eloquent) fn json_value(value: Value) -> DatabaseResult<serde_json::Value> {
    Ok(match value {
        Value::Null => serde_json::Value::Null,
        Value::Json(value) => value,
        Value::Bool(value) => serde_json::Value::from(value),
        Value::Integer(value) => serde_json::Value::from(value),
        Value::Unsigned(value) => serde_json::Value::from(value),
        Value::Real(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or(DatabaseError::InvalidQuery("JSON attribute"))?,
        Value::Text(value) => serde_json::Value::from(value),
        _ => return Err(DatabaseError::InvalidQuery("JSON attribute")),
    })
}

pub(super) fn numeric_equivalent(current: &Value, original: &Value) -> bool {
    fn numeric(value: &Value) -> Option<String> {
        match value {
            Value::Integer(_) | Value::Unsigned(_) | Value::Real(_) => scalar_string(value).ok(),
            Value::Text(text) => numeric_prefix(text)
                .filter(|prefix| {
                    *prefix == text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}'])
                })
                .map(|_| text.clone()),
            _ => None,
        }
    }
    numeric(current).is_some_and(|current| numeric(original).as_ref() == Some(&current))
}

pub(super) fn scalar_string(value: &Value) -> DatabaseResult<String> {
    match value {
        Value::Text(value) => Ok(value.clone()),
        Value::Bool(value) => Ok(if *value { "1" } else { "" }.into()),
        Value::Integer(value) => Ok(value.to_string()),
        Value::Unsigned(value) => Ok(value.to_string()),
        Value::Real(value) if value.is_finite() => Ok(value.to_string()),
        _ => Err(DatabaseError::InvalidQuery("attribute string cast")),
    }
}

const TWO_TO_63: f64 = 9_223_372_036_854_775_808.0;

#[allow(
    clippy::cast_possible_truncation,
    reason = "Eloquent integer casts discard the fractional part"
)]
fn truncate(number: f64) -> i64 {
    number as i64
}

fn integer_string(text: &str) -> i64 {
    let Some(prefix) = numeric_prefix(text) else {
        return 0;
    };
    if let Ok(integer) = prefix.parse() {
        return integer;
    }
    let number = prefix.parse::<f64>().unwrap_or(0.0);
    if !number.is_finite() {
        0
    } else {
        truncate(number)
    }
}

pub(super) fn numeric_prefix(text: &str) -> Option<&str> {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let bytes = text.as_bytes();
    let mut cursor = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    let mut digits = cursor - start;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let fraction = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        digits += cursor - fraction;
    }
    if digits == 0 {
        return None;
    }
    let end = cursor;
    if matches!(bytes.get(cursor), Some(b'e' | b'E')) {
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        let exponent = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == exponent {
            cursor = end;
        }
    }
    Some(&text[..cursor])
}
