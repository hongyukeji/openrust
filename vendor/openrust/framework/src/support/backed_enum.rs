use serde_json::Value;

use crate::http::HttpError;

use super::input_scalars::{decimal_float, double_string, numeric_prefix};

mod sealed {
    pub trait Backing {}

    impl Backing for i64 {}
    impl Backing for String {}
}

/// A string or signed 64-bit integer backing for an input enum.
pub trait EnumBacking: sealed::Backing + Sized {
    /// Converts a request value using PHP's weak scalar argument rules.
    fn from_input(value: &Value) -> Result<Self, HttpError>;
}

/// Maps a backing value to an application enum case.
pub trait BackedEnum: Sized {
    type Backing: EnumBacking;

    fn try_from_backing(value: Self::Backing) -> Option<Self>;
}

impl EnumBacking for i64 {
    fn from_input(value: &Value) -> Result<Self, HttpError> {
        match value {
            Value::Null => Ok(0),
            Value::Bool(value) => Ok(i64::from(*value)),
            Value::Number(number) => number
                .as_i64()
                .or_else(|| weak_integer(number.as_f64()?))
                .ok_or(HttpError::InvalidRequest),
            Value::String(text) => {
                let text = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
                if numeric_prefix(text) != Some(text) {
                    return Err(HttpError::InvalidRequest);
                }
                text.parse::<i64>()
                    .ok()
                    .or_else(|| weak_integer(decimal_float(text)?))
                    .ok_or(HttpError::InvalidRequest)
            }
            Value::Array(_) | Value::Object(_) => Err(HttpError::InvalidRequest),
        }
    }
}

impl EnumBacking for String {
    fn from_input(value: &Value) -> Result<Self, HttpError> {
        match value {
            Value::Null => Ok("0".to_owned()),
            Value::Bool(value) => Ok(i64::from(*value).to_string()),
            Value::String(text) => Ok(text.clone()),
            Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    return Ok(integer.to_string());
                }
                let float = number.as_f64().ok_or(HttpError::InvalidRequest)?;
                Ok(weak_integer(float)
                    .map_or_else(|| double_string(float), |integer| integer.to_string()))
            }
            Value::Array(_) | Value::Object(_) => Err(HttpError::InvalidRequest),
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "PHP truncates finite, in-range floats"
)]
fn weak_integer(value: f64) -> Option<i64> {
    const TWO_TO_63: f64 = 9_223_372_036_854_775_808.0;
    (-TWO_TO_63..TWO_TO_63)
        .contains(&value)
        .then_some(value as i64)
}
