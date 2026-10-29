//! PHP backed enum 的弱类型联合参数适配；业务只定义 backing 到 case 的映射。
use super::InputNumber;
use crate::{Error, Result};
use serde_json::Value;

mod sealed {
    pub trait Backing {}
    impl Backing for String {}
    impl Backing for i64 {}
}

/// 框架支持的 enum backing；仅 String 和 i64 实现。
///
/// sealed 约束确保弱类型转换由框架统一执行，业务使用 [`BackedEnum`] 自定义 case 映射。
pub trait EnumBacking: sealed::Backing + Sized {
    /// 以固定 PHP 弱参数规则转换输入；类型不合法时返回安全 400。
    fn from_input(value: &Value) -> Result<Self>;
}
/// 应用枚举的 backing 映射，不要求序列化能力。
pub trait BackedEnum: Sized {
    /// 枚举的字符串或 64 位整数 backing。
    type Backing: EnumBacking;
    /// 将合法 backing 映射为 case；不存在的 backing 返回 None。
    fn try_from_backing(value: Self::Backing) -> Option<Self>;
}
fn type_error() -> Error {
    Error::custom_with_status(
        http::StatusCode::BAD_REQUEST,
        "input_enum_type_invalid",
        "枚举输入类型无效",
    )
}
#[allow(
    clippy::cast_possible_truncation,
    reason = "已确认值在 i64 范围内，PHP 弱参数截去小数部分"
)]
fn weak_integer(number: InputNumber) -> Result<i64> {
    match number {
        InputNumber::Integer(value) => Ok(value),
        InputNumber::Float(value)
            if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) =>
        {
            Ok(value as i64)
        }
        InputNumber::Float(_) => Err(type_error()),
    }
}
impl EnumBacking for i64 {
    fn from_input(value: &Value) -> Result<Self> {
        match value {
            Value::Null => Ok(0),
            Value::Bool(value) => Ok(i64::from(*value)),
            Value::String(_) | Value::Number(_) => {
                weak_integer(InputNumber::from_value(value).ok_or_else(type_error)?)
            }
            Value::Array(_) | Value::Object(_) => Err(type_error()),
        }
    }
}
impl EnumBacking for String {
    fn from_input(value: &Value) -> Result<Self> {
        match value {
            Value::String(value) => Ok(value.clone()),
            Value::Null => Ok("0".to_owned()),
            Value::Bool(value) => Ok(i64::from(*value).to_string()),
            Value::Number(_) => {
                let number = InputNumber::from_value(value).ok_or_else(type_error)?;
                Ok(weak_string_number(number))
            }
            Value::Array(_) | Value::Object(_) => Err(type_error()),
        }
    }
}

fn weak_string_number(number: InputNumber) -> String {
    weak_integer(number).map_or_else(
        |_| super::scalars::double_string(number.as_float()),
        |value| value.to_string(),
    )
}
#[cfg(test)]
#[path = "enum_golden.rs"]
mod php_runtime_golden;
