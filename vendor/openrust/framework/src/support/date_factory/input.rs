//! 日期输入保留数字与文本的区别，浮点非法值不会被 JSON 悄悄转换成 null。
use crate::support::Date;
use serde_json::Value;
pub enum DateInput {
    Now,
    Date(Date),
    Text(String),
    Integer(i64),
    Unsigned(u64),
    Number(f64),
    Json(Value),
}
impl From<Date> for DateInput {
    fn from(value: Date) -> Self {
        Self::Date(value)
    }
}
impl From<&Date> for DateInput {
    fn from(value: &Date) -> Self {
        Self::Date(value.clone())
    }
}
impl From<&str> for DateInput {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<String> for DateInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<i64> for DateInput {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<i32> for DateInput {
    fn from(value: i32) -> Self {
        Self::Integer(value.into())
    }
}
impl From<u64> for DateInput {
    fn from(value: u64) -> Self {
        Self::Unsigned(value)
    }
}
impl From<f64> for DateInput {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
impl From<Value> for DateInput {
    fn from(value: Value) -> Self {
        Self::Json(value)
    }
}

impl From<&String> for DateInput {
    fn from(value: &String) -> Self {
        Self::Text(value.clone())
    }
}
impl From<u32> for DateInput {
    fn from(value: u32) -> Self {
        Self::Unsigned(value.into())
    }
}

impl DateInput {
    pub(super) fn scalar(self) -> Result<Value, crate::support::DateError> {
        use crate::support::DateError;
        match self {
            Self::Text(value) => Ok(value.into()),
            Self::Integer(value) => Ok(value.into()),
            Self::Unsigned(value) => Ok(value.into()),
            Self::Number(value) => serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or(DateError::OutOfRange),
            Self::Json(value) if value.is_string() || value.is_number() => Ok(value),
            _ => Err(DateError::InvalidExpression),
        }
    }
}
