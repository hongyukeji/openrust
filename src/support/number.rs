//! 固定 PHP 数值判定与两参数 clamp 的类型保留规则。
use serde::{Serialize, Serializer};
use serde_json::Value;

/// 输入整数或浮点数；不把精确整数预先转为浮点数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputNumber {
    /// PHP 平台的 64 位整数。
    Integer(i64),
    /// PHP 双精度数，允许 NaN 和无穷值作为显式边界。
    Float(f64),
}
impl From<i64> for InputNumber {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<f64> for InputNumber {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}
impl Serialize for InputNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Integer(value) => serializer.serialize_i64(*value),
            Self::Float(value) => serializer.serialize_f64(*value),
        }
    }
}
impl InputNumber {
    /// 按固定 PHP 两参数 `min(max(value,min),max)` 选取原参数及其类型。
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        let lower = if self.at_least(min) { self } else { min };
        if lower.less(max) { lower } else { max }
    }
    fn less(self, other: Self) -> bool {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a < b,
            _ => self.as_float() < other.as_float(),
        }
    }
    fn at_least(self, other: Self) -> bool {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a >= b,
            _ => self.as_float() >= other.as_float(),
        }
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "固定 PHP 混合 int/double 比较先转换为 double"
    )]
    pub(crate) fn as_float(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Float(value) => value,
        }
    }
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Number(number) => number
                .as_i64()
                .map(Self::Integer)
                .or_else(|| number.as_f64().map(Self::Float)),
            Value::String(text) => Self::numeric_string(text),
            _ => None,
        }
    }
    pub(crate) fn numeric_string(text: &str) -> Option<Self> {
        let trimmed = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
        let prefix = super::scalars::numeric_prefix(trimmed)?;
        if prefix.len() != trimmed.len() {
            return None;
        }
        prefix
            .parse::<i64>()
            .map(Self::Integer)
            .ok()
            .or_else(|| prefix.parse::<f64>().map(Self::Float).ok())
    }
}

#[cfg(test)]
#[path = "number_golden.rs"]
mod php_runtime_golden;
