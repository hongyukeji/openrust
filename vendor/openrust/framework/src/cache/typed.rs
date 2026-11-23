use std::future::Future;

use serde_json::Value as Json;

use super::{Cache, CacheError, CacheResult, Repository, Value};

macro_rules! typed_reads {
    ($($name:ident, $lazy:ident, $return:ty, $parse:expr, $expected:literal;)+) => {
                impl Repository {
            $(
                #[doc = concat!("Reads a cached ", $expected, " or checks the default on a miss.")]
                pub async fn $name(&self, key: &str, default: Option<Value>) -> CacheResult<$return> {
                    self.$lazy(key, || async { Ok(default.unwrap_or(Value::Json(Json::Null))) }).await
                }

                #[doc = concat!("Reads a cached ", $expected, " with a lazy default.")]
                pub async fn $lazy<F>(&self, key: &str, default: impl FnOnce() -> F) -> CacheResult<$return>
                where
                    F: Future<Output = CacheResult<Value>>,
                {
                    let value = self.get_or_else(key, default).await?;
                    ($parse)(value).ok_or_else(|| CacheError::InvalidType { key: key.into(), expected: $expected })
                }
            )+
        }

                impl Cache {
            $(
                #[doc = concat!("Reads a cached ", $expected, " from the default store.")]
                pub async fn $name(key: &str, default: Option<Value>) -> CacheResult<$return> {
                    Self::store(None)?.$name(key, default).await
                }

                #[doc = concat!("Reads a cached ", $expected, " with a lazy default.")]
                pub async fn $lazy<F>(key: &str, default: impl FnOnce() -> F) -> CacheResult<$return>
                where
                    F: Future<Output = CacheResult<Value>>,
                {
                    Self::store(None)?.$lazy(key, default).await
                }
            )+
        }
    };
}

typed_reads! {
    string, string_or_else, String, |value| match value { Value::Json(Json::String(text)) => Some(text), _ => None }, "string";
    integer, integer_or_else, i64, integer_value, "integer";
    float, float_or_else, f64, float_value, "float";
    boolean, boolean_or_else, bool, |value| match value { Value::Json(Json::Bool(flag)) => Some(flag), _ => None }, "boolean";
    array, array_or_else, Json, |value| match value { Value::Json(value @ (Json::Array(_) | Json::Object(_))) => Some(value), _ => None }, "array";
}

fn integer_value(value: Value) -> Option<i64> {
    match value {
        Value::Json(Json::Number(number)) => number.as_i64().or_else(|| {
            let float = number.as_f64().filter(|_| number.is_f64())?;
            // A PHP float is passed as a decimal string at its default precision.
            // Numbers at this magnitude use exponent notation in PHP's integer validator.
            (float.fract() == 0.0 && float.abs() < 1e14).then_some(float as i64)
        }),
        Value::Json(Json::Bool(true)) => Some(1),
        Value::Json(Json::String(text)) => {
            let text = trim_number(&text);
            let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
            if digits.len() > 1 && digits.starts_with('0') {
                return None;
            }
            text.parse().ok()
        }
        _ => None,
    }
}

fn float_value(value: Value) -> Option<f64> {
    match value {
        Value::Json(Json::Number(number)) => number.as_f64(),
        Value::Json(Json::Bool(true)) => Some(1.0),
        Value::Json(Json::String(text)) => {
            let text = trim_number(&text);
            let number: f64 = text.parse().ok()?;
            if !number.is_finite() {
                return None;
            }
            if number != 0.0 {
                return Some(number);
            }
            let (digits, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
            (exponent.parse::<i64>().ok()? == 0
                && !digits.bytes().any(|digit| matches!(digit, b'1'..=b'9')))
            .then_some(number)
        }
        _ => None,
    }
}

fn trim_number(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\r', '\u{000B}'])
}
