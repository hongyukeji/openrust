//! PHP scalar conversions for request input.

use std::borrow::Cow;

use serde_json::Value;

const TWO_TO_63: f64 = 9_223_372_036_854_775_808.0;
const FILTER_WHITESPACE: &[char] = &[' ', '\t', '\r', '\u{b}', '\n'];

pub(crate) fn string(value: Value) -> String {
    match value {
        Value::Null | Value::Bool(false) => String::new(),
        Value::Bool(true) => "1".to_owned(),
        Value::String(text) => text,
        Value::Number(number) => number.as_i64().map_or_else(
            || double_string(number.as_f64().unwrap_or_default()),
            |integer| integer.to_string(),
        ),
        Value::Array(_) | Value::Object(_) => "Array".to_owned(),
    }
}

pub(crate) fn integer(value: &Value) -> i64 {
    match value {
        Value::Null | Value::Bool(false) => 0,
        Value::Bool(true) => 1,
        Value::String(text) => integer_string(text),
        Value::Number(number) => number
            .as_i64()
            .unwrap_or_else(|| double_integer(number.as_f64().unwrap_or(0.0))),
        Value::Array(values) => i64::from(!values.is_empty()),
        Value::Object(values) => i64::from(!values.is_empty()),
    }
}

/// 输入值的 PHP `(float)` 转换；合法数值前缀的指数溢出可以产生无穷值。
pub(crate) fn float(value: &Value) -> f64 {
    match value {
        Value::Null | Value::Bool(false) => 0.0,
        Value::Bool(true) => 1.0,
        Value::Number(number) => number.as_f64().unwrap_or(0.0),
        Value::String(text) => numeric_prefix(text).and_then(decimal_float).unwrap_or(0.0),
        Value::Array(values) => f64::from(u8::from(!values.is_empty())),
        Value::Object(values) => f64::from(u8::from(!values.is_empty())),
    }
}

pub(crate) fn integer_string(text: &str) -> i64 {
    let Some(prefix) = numeric_prefix(text) else {
        return 0;
    };
    if let Ok(integer) = prefix.parse::<i64>() {
        return integer;
    }
    let number = decimal_float(prefix).unwrap_or(0.0);
    // zend_dval_to_lval_cap：非有限值为 0，有限溢出封顶，不是浮点值的取模。
    if !number.is_finite() {
        0
    } else if number >= TWO_TO_63 {
        i64::MAX
    } else if number < -TWO_TO_63 {
        i64::MIN
    } else {
        double_integer(number)
    }
}

// Zend先截显式指数，再处理尾数/小数位置；提前抵消会令大输入得到不同有限结果。
fn decimal_text(text: &str) -> Cow<'_, str> {
    let Some(index) = text.find(['e', 'E']) else {
        return Cow::Borrowed(text);
    };
    let exponent = &text[index + 1..];
    let magnitude = exponent
        .strip_prefix('+')
        .or_else(|| exponent.strip_prefix('-'))
        .unwrap_or(exponent)
        .trim_start_matches('0');
    if magnitude.len() > 8 || magnitude.parse::<u32>().is_ok_and(|value| value > 19_999) {
        let sign = if exponent.starts_with('-') { "-" } else { "" };
        Cow::Owned(format!("{}e{sign}19999", &text[..index]))
    } else {
        Cow::Borrowed(text)
    }
}

pub(crate) fn decimal_float(text: &str) -> Option<f64> {
    decimal_text(text).parse().ok()
}

pub(crate) fn numeric_prefix(text: &str) -> Option<&str> {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let bytes = text.as_bytes();
    let mut cursor = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    let mut digits = cursor - start;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let fraction = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        digits += cursor - fraction;
    }
    if digits == 0 {
        return None;
    }
    let end = cursor;
    if matches!(bytes.get(cursor), Some(b'e' | b'E')) {
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        let exponent = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == exponent {
            cursor = end;
        }
    }
    Some(&text[..cursor])
}

pub(crate) fn double_integer(number: f64) -> i64 {
    if !number.is_finite() {
        return 0;
    }
    let bits = number.to_bits();
    let bytes = bits.to_be_bytes();
    let exponent = i32::from(u16::from_be_bytes([bytes[0] & 0x7f, bytes[1]]) >> 4) - 1023;
    let significand = (bits & 0x000f_ffff_ffff_ffff) | (1_u64 << 52);
    let magnitude = match exponent {
        0..=51 => significand >> (52 - exponent),
        52..=115 => significand.wrapping_shl(u32::try_from(exponent - 52).unwrap_or(0)),
        _ => return 0,
    };
    let wrapped = if number.is_sign_negative() {
        magnitude.wrapping_neg()
    } else {
        magnitude
    };
    i64::from_ne_bytes(wrapped.to_ne_bytes())
}

// 默认 precision=14；对应 zend_gcvt 的 dtoa(mode=2, ndigit=14) 后布局。
// Rust 科学格式先给出同等有效数字，再按 PHP 的阈值与 E/+/.0 规则排版。
pub(crate) fn double_string(number: f64) -> String {
    if number.is_nan() {
        return "NAN".to_owned();
    }
    if number.is_infinite() {
        return if number.is_sign_negative() {
            "-INF"
        } else {
            "INF"
        }
        .to_owned();
    }
    let scientific = format!("{number:.13e}");
    let (coefficient, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    let (sign, coefficient) = coefficient
        .strip_prefix('-')
        .map_or(("", coefficient), |coefficient| ("-", coefficient));
    let digits = coefficient.replace('.', "");
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    if !(-4..14).contains(&exponent) {
        let fraction = if digits.len() > 1 { &digits[1..] } else { "0" };
        return format!("{sign}{}.{fraction}E{exponent:+}", &digits[..1]);
    }
    let decimal = exponent + 1;
    if decimal <= 0 {
        return format!(
            "{sign}0.{}{digits}",
            "0".repeat(usize::try_from(-decimal).unwrap_or(0))
        );
    }
    let decimal = usize::try_from(decimal).unwrap_or(0);
    if decimal >= digits.len() {
        format!("{sign}{digits}{}", "0".repeat(decimal - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..decimal], &digits[decimal..])
    }
}

pub(crate) fn boolean(value: &Value, default: bool, strict: bool) -> bool {
    if let Value::Bool(value) = value {
        return *value;
    }
    let Some(text) = filter_text(value) else {
        return strict && default;
    };
    if ["1", "true", "on", "yes"]
        .iter()
        .any(|token| text.eq_ignore_ascii_case(token))
    {
        return true;
    }
    strict
        && default
        && !["0", "false", "off", "no"]
            .iter()
            .any(|token| text.eq_ignore_ascii_case(token))
}

pub(crate) fn valid_integer(value: &Value) -> bool {
    let Some(text) = filter_text(value) else {
        return false;
    };
    let digits = text.strip_prefix(['+', '-']).unwrap_or(&text);
    (digits == "0" || !digits.starts_with('0')) && text.parse::<i64>().is_ok()
}

pub(crate) fn valid_float(value: &Value) -> bool {
    let Some(text) = filter_text(value) else {
        return false;
    };
    numeric_prefix(&text) == Some(&text)
        && decimal_float(&text).is_some_and(|number| {
            number.is_finite()
                && (number != 0.0 || !text.bytes().any(|byte| matches!(byte, b'1'..=b'9')))
        })
}

fn filter_text(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::String(text) => Some(Cow::Borrowed(text.trim_matches(FILTER_WHITESPACE))),
        Value::Number(number) => Some(Cow::Owned(number.as_i64().map_or_else(
            || double_string(number.as_f64().unwrap_or_default()),
            |number| number.to_string(),
        ))),
        _ => None,
    }
}
