//! 已解析输入的 PHP 标量转换；缺失值默认与路径限额由 Request 处理。
//!
//! 固定依据为 Laravel v13.39.0 `InteractsWithData` 与 PHP 8.5.0 的
//! `Zend/zend_operators.{c,h}`、`Zend/zend_strtod.c`、`ext/filter/logical_filters.c`。
//! `Value::Object` 在输入 bag 中表示 PHP 关联数组，不表示任意 PHP 对象。

use serde_json::Value;

const TWO_TO_63: f64 = 9_223_372_036_854_775_808.0;

/// 输入值的 `(string)` 转换；数组只返回 Array，不模拟 PHP warning。
pub(crate) fn string(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(false) => String::new(),
        Value::Bool(true) => "1".to_owned(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.as_i64().map_or_else(
            || double_string(number.as_f64().unwrap_or(0.0)),
            |integer| integer.to_string(),
        ),
        Value::Array(_) | Value::Object(_) => "Array".to_owned(),
    }
}

/// 输入值的 64 位 PHP `(int)` 转换；字符串前缀与实际浮点值的溢出规则不同。
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
        Value::String(text) => numeric_prefix(text)
            .and_then(|prefix| prefix.parse().ok())
            .unwrap_or(0.0),
        Value::Array(values) => f64::from(u8::from(!values.is_empty())),
        Value::Object(values) => f64::from(u8::from(!values.is_empty())),
    }
}

/// `FILTER_VALIDATE_BOOLEAN` 转换；大小写不敏感，容器不按非空真值判断。
pub(crate) fn boolean(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Null | Value::Array(_) | Value::Object(_) => false,
        Value::String(text) => boolean_string(text),
        // filter_var 在过滤前将标量转字符串，不能只比较原浮点值是否等于 1。
        Value::Number(_) => boolean_string(&string(value)),
    }
}

fn boolean_string(text: &str) -> bool {
    // PHP_FILTER_TRIM_DEFAULT_EX 不包含 form feed、NUL 或 Unicode whitespace。
    let text = text.trim_matches([' ', '\t', '\r', '\u{b}', '\n']);
    text == "1"
        || text.eq_ignore_ascii_case("true")
        || text.eq_ignore_ascii_case("on")
        || text.eq_ignore_ascii_case("yes")
}

fn integer_string(text: &str) -> i64 {
    let Some(prefix) = numeric_prefix(text) else {
        return 0;
    };
    if let Ok(integer) = prefix.parse::<i64>() {
        return integer;
    }
    let number = prefix.parse::<f64>().unwrap_or(0.0);
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

pub(crate) fn numeric_prefix(text: &str) -> Option<&str> {
    // is_numeric_string 与 zend_strtod 的前导空白是这六个 ASCII 字符。
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

// zend_dval_to_lval 的 64 位结果：非有限值为 0；截去小数后按 2^64 取模。
// 直接读取 IEEE754 位避免 Rust 的浮点 as 整数饱和行为，也避免模后的舍入。
fn double_integer(number: f64) -> i64 {
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

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "固定转换结果按相同 IEEE754 字面值精确比较，不使用误差容限掩盖强转偏差"
)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 检出把 PHP 标量强转误作严格字符串反序列化的实现。
    #[test]
    fn string_keeps_input_text_and_casts_null_booleans_and_arrays() {
        for (value, expected) in [
            (Value::Null, ""),
            (json!(false), ""),
            (json!(true), "1"),
            (json!("  title  "), "  title  "),
            (json!(12), "12"),
            (json!([]), "Array"),
            (json!({}), "Array"),
            (json!({"name": "value"}), "Array"),
        ] {
            assert_eq!(string(&value), expected);
        }
    }

    /// 检出整数转换只接受完整数字、按八进制解释或忽略指数的问题。
    #[test]
    fn integer_uses_decimal_prefix_including_decimal_points_and_exponents() {
        for (value, expected) in [
            ("078", 78),
            (" 901", 901),
            ("nan", 0),
            ("1ab", 1),
            ("2_000", 2),
            (".6", 0),
            ("-.6", 0),
            ("1.ab", 1),
            ("-1.9", -1),
            ("1e3", 1000),
            ("2e+", 2),
            ("+", 0),
            (".abc", 0),
            ("0x10", 0),
            ("1\0later", 1),
        ] {
            assert_eq!(integer(&json!(value)), expected, "{value:?}");
        }
    }

    /// 检出浮点转换错误重用严格解析器或整数前缀的问题。
    #[test]
    fn float_keeps_fraction_and_valid_exponents_but_not_trailing_text() {
        for (value, expected) in [
            (".6", 0.6),
            ("1.ab", 1.0),
            ("1e3", 1000.0),
            ("nan", 0.0),
            ("-1.25suffix", -1.25),
            ("2_000", 2.0),
            (".5e-2!", 0.005),
            ("1e+no", 1.0),
            ("INF", 0.0),
        ] {
            assert_eq!(float(&json!(value)), expected, "{value:?}");
        }
        assert!(float(&json!("1e400")).is_infinite());
        assert!(float(&json!("-0.0")).is_sign_negative());
    }

    /// 检出数值转换误用 Unicode trim；数值自身允许 form feed。
    #[test]
    fn numeric_whitespace_is_ascii_and_does_not_reapply_input_normalization() {
        assert_eq!(integer(&json!("\u{c}12")), 12);
        assert_eq!(float(&json!("\u{c}.5")), 0.5);
        for text in ["\u{a0}12", "\u{200b}12", "\0 12"] {
            assert_eq!(integer(&json!(text)), 0);
            assert_eq!(float(&json!(text)), 0.0);
        }
    }

    /// 检出误把默认值、容器长度或关联数组对象强转规则带入标量转换。
    #[test]
    fn numeric_null_and_containers_cast_to_zero_or_one() {
        for (value, expected) in [
            (Value::Null, 0),
            (json!(false), 0),
            (json!(true), 1),
            (json!([]), 0),
            (json!({}), 0),
            (json!([0, 0]), 1),
            (json!({"a": null}), 1),
        ] {
            assert_eq!(integer(&value), expected);
            assert_eq!(float(&value), if expected == 0 { 0.0 } else { 1.0 });
        }
    }

    /// 检出把 `FILTER_VALIDATE_BOOLEAN` 误作 PHP 真值或 serde bool 的实现。
    #[test]
    fn boolean_uses_filter_words_and_its_own_whitespace_set() {
        for value in [
            json!(true),
            json!(1),
            json!(1.0),
            json!("1"),
            json!(" TrUe "),
            json!("\ton\r"),
            json!("\u{b}YES\n"),
        ] {
            assert!(boolean(&value), "{value:?}");
        }
        for value in [
            Value::Null,
            json!(false),
            json!(0),
            json!(2),
            json!("2"),
            json!("false"),
            json!("off"),
            json!("no"),
            json!("01"),
            json!(""),
            json!("\u{c}true"),
            json!("true\0"),
            json!("\u{a0}yes"),
            json!([]),
            json!([1]),
            json!({"a": true}),
        ] {
            assert!(!boolean(&value), "{value:?}");
        }
        // filter_var 先将浮点数按 precision14 转成 "1"。
        assert!(boolean(&json!(1.000_000_000_000_000_2_f64)));
    }

    /// 检出字符串溢出被误用浮点的模转换，或直接使用 Rust 饱和 as。
    #[test]
    fn string_integer_overflow_caps_finite_values_and_zeroes_infinity() {
        assert_eq!(integer(&json!("9223372036854775807")), i64::MAX);
        assert_eq!(integer(&json!("9223372036854775808")), i64::MAX);
        assert_eq!(integer(&json!("-9223372036854775808")), i64::MIN);
        assert_eq!(integer(&json!("-9223372036854775809")), i64::MIN);
        assert_eq!(integer(&json!("1e20")), i64::MAX);
        assert_eq!(integer(&json!("-1e20")), i64::MIN);
        assert_eq!(integer(&json!("1e400")), 0);
    }

    /// 检出 PHP double 转整数未取模、负数未截断或非有限值未归零。
    #[test]
    fn double_integer_truncates_then_wraps_64_bits() {
        assert_eq!(double_integer(1.9), 1);
        assert_eq!(double_integer(-1.9), -1);
        assert_eq!(double_integer(TWO_TO_63), i64::MIN);
        assert_eq!(integer(&json!(1e20)), 7_766_279_631_452_241_920);
        assert_eq!(double_integer(-TWO_TO_63), i64::MIN);
        assert_eq!(double_integer(18_446_744_073_709_551_616.0), 0);
        assert_eq!(double_integer(18_446_744_073_709_555_712.0), 4096);
        assert_eq!(double_integer(-18_446_744_073_709_555_712.0), -4096);
        assert_eq!(double_integer(f64::INFINITY), 0);
        assert_eq!(double_integer(f64::NAN), 0);
        assert_eq!(integer(&json!(9_223_372_036_854_775_808_u64)), i64::MIN);
    }

    /// 检出默认 precision14、科学格式阈值、指数或负零格式偏离。
    #[test]
    fn double_string_uses_php_precision_and_not_json_number_formatting() {
        for (number, expected) in [
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (1.25, "1.25"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (10_000_000_000_000.0, "10000000000000"),
            (100_000_000_000_000.0, "1.0E+14"),
            (1.234_567_890_123_456, "1.2345678901235"),
            (f64::INFINITY, "INF"),
            (f64::NEG_INFINITY, "-INF"),
            (f64::NAN, "NAN"),
        ] {
            assert_eq!(double_string(number), expected, "{number:?}");
        }
        assert_eq!(
            string(&json!(9_223_372_036_854_775_808_u64)),
            "9.2233720368548E+18"
        );
    }
}

#[cfg(test)]
#[path = "scalar_golden.rs"]
mod php_runtime_golden;
