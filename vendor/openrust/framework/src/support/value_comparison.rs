//! PHP 64-bit scalar equality bridge for compiled framework extensions.
//! Ported from openrust-v0.x support/number.rs; no JSON-to-text comparison shortcut.
use serde_json::Value;
use std::cmp::Ordering;
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
/// 为 PHP 宽松字符串比较筛选可能数值等价的候选。共享数值位模式的字符串仍须逐一比较。
#[doc(hidden)]
#[must_use]
pub fn php_loose_string_numeric_bucket(text: &str) -> Option<u64> {
    let value = InputNumber::numeric_string(text)?.as_float();
    value
        .is_finite()
        .then(|| if value == 0.0 { 0 } else { value.to_bits() })
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

/// 对 JSON 表示的 PHP 标量与数组执行默认排序比较。
/// 混合类型比较可能不满足传递性，调用者须使用可以处理此情况的排序。
#[doc(hidden)]
#[must_use]
pub fn php_regular_compare(left: &Value, right: &Value) -> Ordering {
    use Ordering::{Equal, Greater, Less};

    if left.is_boolean() || right.is_boolean() {
        return super::arr::truthy(left).cmp(&super::arr::truthy(right));
    }
    match (left, right) {
        (Value::Null, Value::String(right)) => "".cmp(right),
        (Value::String(left), Value::Null) => left.as_str().cmp(""),
        (Value::Null, _) | (_, Value::Null) => {
            super::arr::truthy(left).cmp(&super::arr::truthy(right))
        }
        (Value::String(left), Value::String(right)) => compare_numeric_strings(left, right),
        (Value::Number(_), Value::Number(_) | Value::String(_)) => {
            let left_number = InputNumber::from_value(left).expect("JSON number");
            if let Some(right_number) = InputNumber::from_value(right) {
                compare_numbers(left_number, right_number)
            } else {
                super::input_scalars::string(left.clone())
                    .as_str()
                    .cmp(right.as_str().expect("非数值字符串"))
            }
        }
        (Value::String(_), Value::Number(_)) => php_regular_compare(right, left).reverse(),
        (Value::Array(_) | Value::Object(_), Value::Array(_) | Value::Object(_)) => {
            compare_collections(left, right)
        }
        (Value::Array(_) | Value::Object(_), _) => Greater,
        (_, Value::Array(_) | Value::Object(_)) => Less,
        _ => Equal,
    }
}

fn compare_collections(left: &Value, right: &Value) -> Ordering {
    let length = |value: &Value| {
        value
            .as_array()
            .map(Vec::len)
            .or_else(|| value.as_object().map(serde_json::Map::len))
            .unwrap_or_default()
    };
    let order = length(left).cmp(&length(right));
    if !order.is_eq() {
        return order;
    }
    let compare = |key: &str, value: &Value| {
        super::data::direct(right, key)
            .map_or(Ordering::Greater, |other| php_regular_compare(value, other))
    };
    match left {
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, value)| compare(&index.to_string(), value))
            .find(|order| !order.is_eq()),
        Value::Object(items) => items
            .iter()
            .map(|(key, value)| compare(key, value))
            .find(|order| !order.is_eq()),
        _ => None,
    }
    .unwrap_or(Ordering::Equal)
}

fn compare_numeric_strings(left: &str, right: &str) -> Ordering {
    let (Some(left_number), Some(right_number)) = (
        InputNumber::numeric_string(left),
        InputNumber::numeric_string(right),
    ) else {
        return left.cmp(right);
    };
    let left_overflow = numeric_string_overflow(left, left_number);
    let right_overflow = numeric_string_overflow(right, right_number);
    match (left_number, right_number) {
        (InputNumber::Integer(_), InputNumber::Float(_)) if right_overflow != 0 => {
            0.cmp(&right_overflow)
        }
        (InputNumber::Float(_), InputNumber::Integer(_)) if left_overflow != 0 => {
            left_overflow.cmp(&0)
        }
        (InputNumber::Float(a), InputNumber::Float(b))
            if a.partial_cmp(&b) == Some(Ordering::Equal)
                && (!a.is_finite() || (left_overflow != 0 && left_overflow == right_overflow)) =>
        {
            left.cmp(right)
        }
        _ => compare_numbers(left_number, right_number),
    }
}

#[path = "regular_string_sort.rs"]
mod regular_string_sort;

/// 保留固定 PHP `array_unique(..., SORT_REGULAR)` 对字符串数组的值与原始顺序。
#[doc(hidden)]
#[must_use]
pub fn php_unique_regular_strings(values: Vec<String>) -> Vec<String> {
    if values.len() < 2 {
        return values;
    }
    let mut sorted: Vec<_> = (0..values.len()).collect();
    let compare =
        |left: usize, right: usize| compare_numeric_strings(&values[left], &values[right]);
    regular_string_sort::sort(&mut sorted, |left, right| compare(left, right).is_gt());
    let mut keep = vec![true; values.len()];
    let mut first = sorted[0];
    for &next in &sorted[1..] {
        if compare(first, next).is_eq() {
            keep[first.max(next)] = false;
            first = first.min(next);
        } else {
            first = next;
        }
    }
    values
        .into_iter()
        .zip(keep)
        .filter_map(|(value, keep)| keep.then_some(value))
        .collect()
}

fn compare_numbers(left: InputNumber, right: InputNumber) -> Ordering {
    match (left, right) {
        (InputNumber::Integer(left), InputNumber::Integer(right)) => left.cmp(&right),
        (left, right) => left
            .as_float()
            .partial_cmp(&right.as_float())
            .unwrap_or(Ordering::Equal),
    }
}

/// PHP default precision=14 scalar conversion, shared with compiled extensions.
#[doc(hidden)]
#[must_use]
pub fn php_float_string(value: f64) -> String {
    super::input_scalars::double_string(value)
}

/// JSON_NUMERIC_CHECK for a UTF-8 string, using the existing PHP numeric parser.
#[doc(hidden)]
#[must_use]
pub fn php_numeric_json_string(value: &str, preserve_zero_fraction: bool) -> Option<String> {
    match InputNumber::numeric_string(value)? {
        InputNumber::Integer(integer) => Some(integer.to_string()),
        InputNumber::Float(float) if float.is_finite() => {
            Some(json_float_string(float, preserve_zero_fraction))
        }
        InputNumber::Float(_) => None,
    }
}

fn json_float_string(value: f64, preserve_zero_fraction: bool) -> String {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0{}", if preserve_zero_fraction { ".0" } else { "" });
    }
    // serde supplies shortest round-trip digits; Zend's JSON layout uses its own thresholds.
    let shortest = serde_json::to_string(&value.abs()).expect("finite float");
    let (coefficient, power) = shortest.split_once(['e', 'E']).unwrap_or((&shortest, "0"));
    let power = power.parse::<i32>().unwrap_or(0);
    let decimal = coefficient.find('.').unwrap_or(coefficient.len());
    let raw = coefficient.replace('.', "");
    let leading = raw.len() - raw.trim_start_matches('0').len();
    let digits = raw.trim_start_matches('0').trim_end_matches('0');
    let exponent =
        power + i32::try_from(decimal).unwrap_or(0) - 1 - i32::try_from(leading).unwrap_or(0);
    if !(-4..17).contains(&exponent) {
        return format!(
            "{sign}{}.{}e{exponent:+}",
            &digits[..1],
            if digits.len() > 1 { &digits[1..] } else { "0" }
        );
    }
    let point = exponent + 1;
    let mut output = if point <= 0 {
        format!(
            "{sign}0.{}{digits}",
            "0".repeat(usize::try_from(-point).unwrap_or(0))
        )
    } else {
        let point = usize::try_from(point).unwrap_or(0);
        if point >= digits.len() {
            format!("{sign}{digits}{}", "0".repeat(point - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..point], &digits[point..])
        }
    };
    if preserve_zero_fraction && !output.contains('.') {
        output.push_str(".0");
    }
    output
}

/// 固定 PHP 的 int < JSON value，用于扩展中未经验证的请求比较。
/// JSON 对象按关联数组处理；不承载 PHP 对象或资源。
#[doc(hidden)]
#[must_use]
pub fn php_integer_less_than_value(left: i64, right: &Value) -> bool {
    match right {
        Value::Null => false,
        Value::Bool(right) => left == 0 && *right,
        Value::Array(_) | Value::Object(_) => true,
        _ => match InputNumber::from_value(right) {
            Some(InputNumber::Integer(right)) => left < right,
            Some(right) => InputNumber::Integer(left).as_float() < right.as_float(),
            None => right
                .as_str()
                .is_some_and(|right| left.to_string().as_str() < right),
        },
    }
}

/// 固定 PHP 的 (int) JSON 输入转换，供编译型扩展复用。
#[doc(hidden)]
#[must_use]
pub fn php_integer_cast(value: &Value) -> i64 {
    super::input_scalars::integer(value)
}

/// 固定 PHP 的 (float) JSON 输入转换，供编译型扩展复用。
#[doc(hidden)]
#[must_use]
pub fn php_float_cast(value: &Value) -> f64 {
    super::input_scalars::float(value)
}

/// JSON 标量或 PHP 关联数组的布尔转换，供编译型扩展复用。
#[doc(hidden)]
#[must_use]
pub fn php_value_truthy(value: &Value) -> bool {
    super::arr::truthy(value)
}

/// 复用固定 PHP 的中点远离零舍入，倍率为 10 的整数次幂。
/// 比较原值与舍入中点，避免先乘后 round 造成可观察的二进制中点偏差。
#[allow(
    clippy::float_cmp,
    reason = "PHP half-up 算法以精确 IEEE754 比较识别可逆整数，不用 epsilon 改变边界"
)]
#[doc(hidden)]
#[must_use]
pub fn php_round_half_up(value: f64, exponent: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let mut integral = (value * exponent).trunc();
    let corrected = integral + 1.0_f64.copysign(value);
    if corrected / exponent == value {
        integral = corrected;
    }
    if integral.abs() >= 1e16 {
        return value;
    }
    let edge = ((integral + 0.5_f64.copysign(integral)) / exponent).abs();
    if value.abs() >= edge {
        integral += 1.0_f64.copysign(integral);
    }
    integral / exponent
}

/// 校验规则复用同一标量比较桥；JSON 数组及映射都表示 PHP 数组。
pub(crate) fn php_loose_value_equal(left: &Value, right: &Value) -> bool {
    if left.is_boolean() || right.is_boolean() {
        return super::arr::truthy(left) == super::arr::truthy(right);
    }
    if left.is_null() || right.is_null() {
        let other = if left.is_null() { right } else { left };
        return other
            .as_str()
            .map_or_else(|| !super::arr::truthy(other), str::is_empty);
    }
    match (left, right) {
        (Value::String(left), right) => php_loose_string_value_equal(left, right),
        (left, Value::String(right)) => php_loose_string_value_equal(right, left),
        (Value::Number(_), Value::Number(_)) => match (
            InputNumber::from_value(left),
            InputNumber::from_value(right),
        ) {
            (Some(InputNumber::Integer(left)), Some(InputNumber::Integer(right))) => left == right,
            (Some(left), Some(right)) => left.as_float() == right.as_float(),
            _ => false,
        },
        (Value::Array(_) | Value::Object(_), Value::Array(_) | Value::Object(_)) => {
            fn entries(value: &Value) -> Vec<(String, &Value)> {
                match value {
                    Value::Array(items) => items
                        .iter()
                        .enumerate()
                        .map(|(key, value)| (key.to_string(), value))
                        .collect(),
                    Value::Object(items) => items
                        .iter()
                        .map(|(key, value)| (key.clone(), value))
                        .collect(),
                    _ => Vec::new(),
                }
            }
            let left = entries(left);
            let right = entries(right);
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .iter()
                        .any(|(other, item)| key == other && php_loose_value_equal(value, item))
                })
        }
        _ => false,
    }
}
