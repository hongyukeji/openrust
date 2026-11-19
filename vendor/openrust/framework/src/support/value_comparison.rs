//! PHP 64-bit scalar equality bridge for compiled framework extensions.
//! Ported from openrust-v0.x support/number.rs; no JSON-to-text comparison shortcut.
use serde_json::Value;
#[derive(Clone, Copy)]
enum InputNumber {
    Integer(i64),
    Float(f64),
}
impl InputNumber {
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
        let whitespace = [' ', '\t', '\n', '\r', '\u{b}', '\u{c}'];
        let leading_trimmed = text.trim_start_matches(whitespace);
        let trimmed = leading_trimmed.trim_end_matches(whitespace);
        let prefix = super::input_scalars::numeric_prefix(trimmed)?;
        if prefix.len() != trimmed.len() {
            return None;
        }
        // Zend 的 19 位边界以原字节串比较 LONG_MIN 幅值，尾空白参与该次比较。
        if let Ok(integer) = prefix.parse::<i64>()
            && (integer != i64::MIN || leading_trimmed.len() == trimmed.len())
        {
            return Some(Self::Integer(integer));
        }
        super::input_scalars::decimal_float(prefix).map(Self::Float)
    }
}
/// 按固定 PHP 64 位智能字符串比较判断宽松相等，保留整数溢出与无穷值的文本回退。
/// 仅供独立扩展复用运行时语义；应用使用其领域接口。
#[doc(hidden)]
#[must_use]
pub fn php_loose_string_equal(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let (Some(left_number), Some(right_number)) = (
        InputNumber::numeric_string(left),
        InputNumber::numeric_string(right),
    ) else {
        return false;
    };
    let left_overflow = numeric_string_overflow(left, left_number);
    let right_overflow = numeric_string_overflow(right, right_number);
    let left_double = left_number.as_float();
    let right_double = right_number.as_float();
    // 同向整数溢出若落在同一 double 上，Zend 使用原始字节而不是合并标识符。
    if left_overflow != 0
        && left_overflow == right_overflow
        && (left_double - right_double).partial_cmp(&0.0) == Some(std::cmp::Ordering::Equal)
    {
        return false;
    }
    match (left_number, right_number) {
        (InputNumber::Integer(left), InputNumber::Integer(right)) => left == right,
        (InputNumber::Integer(_), InputNumber::Float(_)) if right_overflow != 0 => false,
        (InputNumber::Float(_), InputNumber::Integer(_)) if left_overflow != 0 => false,
        _ => {
            left_double.is_finite()
                && right_double.is_finite()
                && left_double.partial_cmp(&right_double) == Some(std::cmp::Ordering::Equal)
        }
    }
}

/// 固定 PHP 字符串与 JSON 标量的宽松比较，仅供扩展互操作。
///
/// String 复用智能字符串比较；number/string 按 Zend 的独立分支比较数值，
/// 仅非 numeric 字符串回退到同一标量格式化。JSON Number 不保留原始数值字面量，
/// 不承载非有限值、PHP对象及整数域外的精确 PHP int；此桥不能恢复解析前的信息。
#[doc(hidden)]
#[must_use]
pub fn php_loose_string_value_equal(needle: &str, value: &Value) -> bool {
    match value {
        Value::String(value) => php_loose_string_equal(needle, value),
        Value::Bool(value) => *value == (!needle.is_empty() && needle != "0"),
        Value::Null => needle.is_empty(),
        Value::Number(_) => {
            // Zend zend_operators.c compare_long_to_string / compare_double_to_string：
            // numeric needle 才转数值；不使用字符串智能比较的整数溢出文本回退。
            let Some(number) = InputNumber::from_value(value) else {
                return false;
            };
            match InputNumber::numeric_string(needle) {
                Some(InputNumber::Integer(left)) if matches!(number, InputNumber::Integer(_)) => {
                    matches!(number, InputNumber::Integer(right) if left == right)
                }
                Some(left) => {
                    left.as_float().partial_cmp(&number.as_float())
                        == Some(std::cmp::Ordering::Equal)
                }
                None => super::input_scalars::string(value.clone()) == needle,
            }
        }
        Value::Array(_) | Value::Object(_) => false,
    }
}

fn numeric_string_overflow(text: &str, number: InputNumber) -> i8 {
    if matches!(number, InputNumber::Integer(_)) {
        return 0;
    }
    let trimmed = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let digits = trimmed
        .trim_start_matches(['+', '-'])
        .bytes()
        .take_while(u8::is_ascii_digit)
        .skip_while(|byte| *byte == b'0')
        .count();
    let fraction_or_exponent = trimmed
        .bytes()
        .any(|byte| matches!(byte, b'.' | b'e' | b'E'));
    // 20 个有效整数位先触发 Zend 溢出标记，后续负指数不会撤销该标记。
    if digits >= 20 || !fraction_or_exponent {
        if trimmed.starts_with('-') { -1 } else { 1 }
    } else {
        0
    }
}
