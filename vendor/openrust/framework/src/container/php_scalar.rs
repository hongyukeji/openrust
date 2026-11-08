//! 已选构造实参的 PHP 弱标量物化。应用诊断策略由调用方在同一槽同步执行。
use super::{ParameterValue, container_error};
use crate::Result;
use crate::support::{InputNumber, scalars};
use std::sync::Arc;

/// 构造参数显式采用的 PHP 弱标量目标。
///
/// 输入可为 `i64`、`f64`、`bool`、`String` 或 [`PhpBytes`]。
/// 可空参数额外接受显式空值，字节串目标额外接受 [`PhpStringable`]。
#[derive(Clone, Copy)]
pub enum PhpTarget {
    /// 对应 PHP `int`，输出 `i64`。
    Int,
    /// 对应 PHP `float`，输出 `f64`。
    Float,
    /// 对应 PHP `string`，输出 [`PhpBytes`]，保留非 UTF-8 字节。
    Bytes,
    /// 对应 PHP `bool`，输出 `bool`。
    Bool,
    /// 对应 PHP `int|float`，输出 [`InputNumber`]。
    Number,
    /// 对应 PHP `int|bool`，输出 [`PhpIntBool`]。
    IntBool,
}

/// PHP 字符串的原始字节，可包含空字节和非 UTF-8 字节。
pub struct PhpBytes(pub(super) Arc<[u8]>);

impl PhpBytes {
    /// 从原始字节构造字符串。已有的共享字节可直接传入。
    #[must_use]
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self(bytes.into())
    }

    /// 借用原始字节，不执行编码转换。
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// PHP `int|bool` 转换后实际选中的类型和值。
#[derive(Debug, Eq, PartialEq)]
pub enum PhpIntBool {
    /// 整数分支。
    Int(i64),
    /// 布尔分支。
    Bool(bool),
}

/// 显式提供 PHP 字符串转换回调。
///
/// 构造参数选择完成后，只有 [`PhpTarget::Bytes`] 会调用此回调。
pub struct PhpStringable(pub(super) Box<dyn Fn() -> Result<PhpBytes> + Send + Sync>);

impl PhpStringable {
    /// 登记同步转换回调。转换错误会终止本次构造。
    pub fn new(convert: impl Fn() -> Result<PhpBytes> + Send + Sync + 'static) -> Self {
        Self(Box::new(convert))
    }
}

/// 在构造参数物化时同步交给应用策略的诊断，不包含实参值。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarDiagnostic {
    /// `E_DEPRECATED`，原实参为浮点数。
    FloatLosesPrecision,
    /// `E_DEPRECATED`，原实参为 numeric string。
    NumericStringLosesPrecision,
    /// `E_WARNING`，NaN 被转换为 PHP 字符串。
    NanToBytes,
    /// `E_WARNING`，NaN 被转换为布尔值，含 int|bool 的布尔分支。
    NanToBool,
}

struct Coerced<T> {
    value: T,
    diagnostic: Option<ScalarDiagnostic>,
}

impl<T> Coerced<T> {
    fn plain(value: T) -> Self {
        Self {
            value,
            diagnostic: None,
        }
    }

    fn map<U>(self, map: impl FnOnce(T) -> U) -> Coerced<U> {
        Coerced {
            value: map(self.value),
            diagnostic: self.diagnostic,
        }
    }
}

impl PhpTarget {
    pub(super) fn materialize(
        self,
        value: &ParameterValue,
        diagnostic: &mut impl FnMut(ScalarDiagnostic) -> Result<()>,
    ) -> Result<ParameterValue> {
        if matches!(self, Self::Bytes) {
            if value.downcast_ref::<PhpBytes>().is_some() {
                return Ok(value.clone());
            }
            if let Some(stringable) = value.downcast_ref::<PhpStringable>() {
                return (stringable.0)().map(ParameterValue::new);
            }
        }

        let input = Scalar::from_value(value).ok_or_else(type_mismatch)?;
        let converted = match self {
            Self::Int => input.to_int().map(|value| value.map(ParameterValue::new)),
            Self::Float => input
                .number()
                .map(|value| Coerced::plain(ParameterValue::new(value.as_float()))),
            Self::Bytes => Some(input.to_bytes().map(ParameterValue::new)),
            Self::Bool => Some(input.to_bool().map(ParameterValue::new)),
            Self::Number => input
                .number()
                .map(|value| Coerced::plain(ParameterValue::new(value))),
            Self::IntBool => Some(input.to_int_bool().map(ParameterValue::new)),
        }
        .ok_or_else(type_mismatch)?;

        // 此处目标已匹配。诊断失败保留原错误，union 不再尝试其它分支。
        if let Some(event) = converted.diagnostic {
            diagnostic(event)?;
        }
        Ok(converted.value)
    }
}

enum Scalar<'a> {
    Int(i64),
    Float(f64),
    Bool(bool),
    Bytes(&'a [u8]),
}

impl<'a> Scalar<'a> {
    fn from_value(value: &'a ParameterValue) -> Option<Self> {
        if let Some(&value) = value.downcast_ref::<i64>() {
            Some(Self::Int(value))
        } else if let Some(&value) = value.downcast_ref::<f64>() {
            Some(Self::Float(value))
        } else if let Some(&value) = value.downcast_ref::<bool>() {
            Some(Self::Bool(value))
        } else if let Some(value) = value.downcast_ref::<PhpBytes>() {
            Some(Self::Bytes(&value.0))
        } else {
            value
                .downcast_ref::<String>()
                .map(|value| Self::Bytes(value.as_bytes()))
        }
    }

    fn number(&self) -> Option<InputNumber> {
        match *self {
            Self::Int(value) => Some(InputNumber::Integer(value)),
            Self::Float(value) => Some(InputNumber::Float(value)),
            Self::Bool(value) => Some(InputNumber::Integer(i64::from(value))),
            Self::Bytes(value) => numeric_bytes(value),
        }
    }

    fn to_int(&self) -> Option<Coerced<i64>> {
        match *self {
            Self::Int(value) => Some(Coerced::plain(value)),
            Self::Bool(value) => Some(Coerced::plain(i64::from(value))),
            Self::Float(value) => truncate_int(value, ScalarDiagnostic::FloatLosesPrecision),
            Self::Bytes(value) => match numeric_bytes(value)? {
                InputNumber::Integer(value) => Some(Coerced::plain(value)),
                InputNumber::Float(value) => {
                    truncate_int(value, ScalarDiagnostic::NumericStringLosesPrecision)
                }
            },
        }
    }

    fn to_int_bool(&self) -> Coerced<PhpIntBool> {
        match *self {
            Self::Bool(value) => Coerced::plain(PhpIntBool::Bool(value)),
            _ => self.to_int().map_or_else(
                || self.to_bool().map(PhpIntBool::Bool),
                |value| value.map(PhpIntBool::Int),
            ),
        }
    }

    fn to_bytes(&self) -> Coerced<PhpBytes> {
        match *self {
            Self::Bytes(bytes) => Coerced::plain(PhpBytes(Arc::from(bytes))),
            Self::Int(value) => Coerced::plain(PhpBytes(Arc::from(value.to_string().into_bytes()))),
            Self::Bool(true) => Coerced::plain(PhpBytes(Arc::from(&b"1"[..]))),
            Self::Bool(false) => Coerced::plain(PhpBytes(Arc::from(&b""[..]))),
            Self::Float(value) => Coerced {
                value: PhpBytes(Arc::from(scalars::double_string(value).into_bytes())),
                diagnostic: value.is_nan().then_some(ScalarDiagnostic::NanToBytes),
            },
        }
    }

    fn to_bool(&self) -> Coerced<bool> {
        match *self {
            Self::Int(value) => Coerced::plain(value != 0),
            Self::Bool(value) => Coerced::plain(value),
            Self::Bytes(value) => Coerced::plain(!value.is_empty() && value != b"0"),
            Self::Float(value) if value.is_nan() => Coerced {
                value: true,
                diagnostic: Some(ScalarDiagnostic::NanToBool),
            },
            Self::Float(value) => Coerced::plain(value.abs() > 0.0),
        }
    }
}

fn numeric_bytes(value: &[u8]) -> Option<InputNumber> {
    std::str::from_utf8(value)
        .ok()
        .and_then(InputNumber::numeric_string)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "有限半开域验证后按 PHP 规则向零截断为 i64"
)]
fn truncate_int(value: f64, precision_loss: ScalarDiagnostic) -> Option<Coerced<i64>> {
    const TWO_TO_63: f64 = 9_223_372_036_854_775_808.0;
    if !(-TWO_TO_63..TWO_TO_63).contains(&value) {
        return None;
    }
    Some(Coerced {
        value: value.trunc() as i64,
        diagnostic: (value.fract().abs() > 0.0).then_some(precision_loss),
    })
}

fn type_mismatch() -> crate::Error {
    container_error(
        "container_parameter_type_mismatch",
        "已选构造实参不能转换为声明的 PHP 标量",
    )
}
