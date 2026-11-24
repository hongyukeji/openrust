//! 严格配置读取共用转换，不使用请求输入的宽松转换规则。
use super::{ConfigError, Value};
use crate::contracts::config::Repository;
use crate::support::Collection;
pub(crate) fn invalid(key: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.into(),
        expected,
    }
}
pub(crate) fn read<R: Repository + ?Sized, T>(
    repository: &R,
    key: &str,
    expected: &'static str,
    default: impl FnOnce() -> Result<T, ConfigError>,
    convert: impl FnOnce(Value) -> Option<T>,
) -> Result<T, ConfigError> {
    match repository.get(key) {
        Some(value) => convert(value).ok_or_else(|| invalid(key, expected)),
        None => default(),
    }
}
pub(crate) fn string(value: Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value),
        _ => None,
    }
}
pub(crate) fn integer(value: Value) -> Option<i64> {
    value.as_i64()
}
pub(crate) fn float(value: Value) -> Option<f64> {
    match value {
        Value::Number(number) if number.is_f64() => number.as_f64(),
        _ => None,
    }
}
pub(crate) fn boolean(value: Value) -> Option<bool> {
    value.as_bool()
}
pub(crate) fn array(value: Value) -> Option<Value> {
    match value {
        Value::Array(_) | Value::Object(_) => Some(value),
        _ => None,
    }
}
pub(crate) fn collection(value: Value) -> Option<Collection> {
    match value {
        Value::Array(items) => Some(items.into_iter().collect()),
        Value::Object(items) => Some(Collection::from_entries(items)),
        _ => None,
    }
}
