use std::future::Future;

use serde_json::Value as Json;

use super::{Cache, CacheError, CacheResult, Repository, Value};

macro_rules! typed_reads {
    ($($name:ident, $or:ident, $lazy:ident, $return:ty, $default:ty, $encode:path, $parse:expr, $expected:literal;)+) => {
        impl Repository {
            $(
                #[doc = concat!("读取并验证缓存 ", $expected, "；缺失、空值或类型不符时报错。")]
                pub async fn $name(&self, key: &str) -> CacheResult<$return> {
                    self.$lazy(key, || async { Ok(Value::Json(Json::Null)) }).await
                }

                #[doc = concat!("读取缓存 ", $expected, "，只在缺失或空值时使用默认值。")]
                pub async fn $or(&self, key: &str, default: $default) -> CacheResult<$return> {
                    self.$lazy(key, || async { $encode(default).map_err(|_| CacheError::InvalidType { key: key.to_owned(), expected: $expected }) }).await
                }

                #[doc = concat!("读取缓存 ", $expected, "，只在缺失或空值时执行默认回调。")]
                pub async fn $lazy<F>(&self, key: &str, default: impl FnOnce() -> F) -> CacheResult<$return>
                where F: Future<Output = CacheResult<Value>> {
                    let value = self.get_or_else(key, default).await?;
                    ($parse)(value).ok_or_else(|| CacheError::InvalidType { key: key.into(), expected: $expected })
                }
            )+
        }

        impl Cache {
            $(
                pub async fn $name(key: &str) -> CacheResult<$return> {
                    Self::store(None)?.$name(key).await
                }
                pub async fn $or(key: &str, default: $default) -> CacheResult<$return> {
                    Self::store(None)?.$or(key, default).await
                }
                pub async fn $lazy<F>(key: &str, default: impl FnOnce() -> F) -> CacheResult<$return>
                where F: Future<Output = CacheResult<Value>> {
                    Self::store(None)?.$lazy(key, default).await
                }
            )+
        }
    };
}

typed_reads! {
    string, string_or, string_or_else, String, impl Into<String>, string_default, |value| match value { Value::Json(Json::String(text)) => Some(text), _ => None }, "string";
    integer, integer_or, integer_or_else, i64, i64, integer_default, integer_value, "integer";
    float, float_or, float_or_else, f64, f64, float_default, float_value, "float";
    boolean, boolean_or, boolean_or_else, bool, bool, boolean_default, |value| match value { Value::Json(Json::Bool(flag)) => Some(flag), _ => None }, "boolean";
    array, array_or, array_or_else, Json, Json, array_default, |value| match value { Value::Json(value @ (Json::Array(_) | Json::Object(_))) => Some(value), _ => None }, "array";
}

fn string_default(value: impl Into<String>) -> CacheResult<Value> {
    Ok(Value::Json(value.into().into()))
}
fn integer_default(value: i64) -> CacheResult<Value> {
    Ok(value.into())
}
fn boolean_default(value: bool) -> CacheResult<Value> {
    Ok(Value::Json(value.into()))
}
fn array_default(value: Json) -> CacheResult<Value> {
    Ok(Value::Json(value))
}
fn float_default(value: f64) -> CacheResult<Value> {
    serde_json::Number::from_f64(value)
        .map(|value| Value::Json(value.into()))
        .ok_or(CacheError::InvalidValue)
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
