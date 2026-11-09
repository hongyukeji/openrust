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
        let whitespace = [' ', '\t', '\n', '\r', '\u{b}', '\u{c}'];
        let leading_trimmed = text.trim_start_matches(whitespace);
        let trimmed = leading_trimmed.trim_end_matches(whitespace);
        let prefix = super::scalars::numeric_prefix(trimmed)?;
        if prefix.len() != trimmed.len() {
            return None;
        }
        // Zend 的 19 位边界以原字节串比较 LONG_MIN 幅值，尾空白参与该次比较。
        if let Ok(integer) = prefix.parse::<i64>()
            && (integer != i64::MIN || leading_trimmed.len() == trimmed.len())
        {
            return Some(Self::Integer(integer));
        }
        super::scalars::decimal_text(prefix)
            .parse::<f64>()
            .map(Self::Float)
            .ok()
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
                None => super::scalars::string(value) == needle,
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

#[cfg(test)]
#[path = "number_golden.rs"]
mod php_runtime_golden;

#[cfg(test)]
mod loose_string_tests {
    use super::php_loose_string_equal;

    #[test]
    fn numeric_strings_follow_full_grammar_and_keep_integer_precision() {
        for (left, right) in [
            ("01", "1"),
            ("1e3", "1000"),
            (" \t+1.0\n", "1"),
            ("-0", "0"),
            (".5", "5e-1"),
        ] {
            assert!(php_loose_string_equal(left, right), "{left:?}, {right:?}");
            assert!(php_loose_string_equal(right, left));
        }
        assert!(!php_loose_string_equal(
            "9007199254740992",
            "9007199254740993"
        ));
        assert!(!php_loose_string_equal(
            "9223372036854775807",
            "9223372036854775806"
        ));
    }

    #[test]
    fn overflowing_integer_strings_do_not_collapse_to_one_float() {
        for (left, right) in [
            ("9223372036854775808", "9223372036854775809"),
            ("-9223372036854775809", "-9223372036854775810"),
            ("9223372036854775807", "9223372036854775808"),
            ("-9223372036854775808", "-9223372036854775809"),
            ("+9223372036854775808", "9223372036854775808"),
            ("9223372036854775808 ", "9223372036854775808"),
            ("100000000000000000000e-20", "1"),
        ] {
            assert!(!php_loose_string_equal(left, right), "{left:?}, {right:?}");
            assert!(!php_loose_string_equal(right, left));
        }
        assert!(php_loose_string_equal(
            "9223372036854775808",
            "9223372036854775808"
        ));
        assert!(php_loose_string_equal(
            "9223372036854775808e0",
            "9223372036854775808"
        ));
    }

    #[test]
    fn non_finite_numeric_results_use_original_text_instead_of_infinity_equality() {
        for (left, right) in [
            ("1e9999", "2e9999"),
            ("-1e9999", "-2e9999"),
            ("1e9999", "1e10000"),
            ("1e9999", "INF"),
            ("-1e9999", "-INF"),
        ] {
            assert!(!php_loose_string_equal(left, right), "{left:?}, {right:?}");
            assert!(!php_loose_string_equal(right, left));
        }
        assert!(php_loose_string_equal("1e9999", "1e9999"));
        assert!(php_loose_string_equal("-1e9999", "-1e9999"));
        assert!(php_loose_string_equal("1e-9999", "2e-9999"));
    }

    #[test]
    fn non_numeric_and_unicode_values_require_exact_text() {
        for (left, right) in [
            ("1suffix", "1"),
            ("1e", "1"),
            ("0x10", "16"),
            ("１", "1"),
            ("\u{a0}1", "1"),
            ("订单", "订"),
            ("NAN", "nan"),
        ] {
            assert!(!php_loose_string_equal(left, right));
            assert!(!php_loose_string_equal(right, left));
        }
        assert!(php_loose_string_equal("订单", "订单"));
        assert!(php_loose_string_equal("NAN", "NAN"));
    }

    #[test]
    fn loose_string_comparison_matches_recorded_php_oracle() -> Result<(), serde_json::Error> {
        // 实际 PHP 输出固定溢出与数值别名语义，避免两个 Rust 调用方共享同一错误。
        #[derive(serde::Deserialize)]
        struct Oracle {
            comparisons: Vec<(String, String, bool)>,
        }
        let fixture: Oracle = serde_json::from_str(include_str!(
            "../../tests/fixtures/passport_string_comparisons.json"
        ))?;
        for (left, right, expected) in fixture.comparisons {
            assert_eq!(
                php_loose_string_equal(&left, &right),
                expected,
                "{left:?}, {right:?}"
            );
            assert_eq!(
                php_loose_string_equal(&right, &left),
                expected,
                "反向 {right:?}, {left:?}"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod exponent_tests {
    use super::InputNumber;

    #[test]
    fn full_numeric_strings_use_capped_exponents_without_losing_integer_priority() {
        for (text, expected) in [
            (format!("0.{}1e20000", "0".repeat(19_999)), 0.1_f64),
            (format!("1{}e-20000", "0".repeat(20_000)), 10.0_f64),
        ] {
            assert!(
                matches!(InputNumber::numeric_string(&text), Some(InputNumber::Float(value)) if value.to_bits() == expected.to_bits())
            );
            assert!(
                matches!(InputNumber::numeric_string(&format!("-{text}")), Some(InputNumber::Float(value)) if value.to_bits() == (-expected).to_bits())
            );
            assert!(InputNumber::numeric_string(&format!("{text}suffix")).is_none());
        }
        assert!(matches!(
            InputNumber::numeric_string("9223372036854775807"),
            Some(InputNumber::Integer(i64::MAX))
        ));
    }
}

// 此测试随既有宽松比较WIP验收，不混入仅含通用数值入口的独立候选。
#[cfg(test)]
mod loose_exponent_tests {
    use super::php_loose_string_equal;

    #[test]
    fn loose_comparison_keeps_original_integer_overflow_category_after_capping() {
        let a = format!("0.{}1e20000", "0".repeat(19_999));
        let b = format!("1{}e-20000", "0".repeat(20_000));
        for (left, right, expected) in [
            (a.as_str(), "0.1", true),
            (a.as_str(), "1", false),
            (b.as_str(), "1e1", true),
            (b.as_str(), "1e0", false),
            (b.as_str(), "10", false),
        ] {
            assert_eq!(php_loose_string_equal(left, right), expected);
            assert_eq!(php_loose_string_equal(right, left), expected);
        }
    }
}

#[cfg(test)]
mod loose_value_tests {
    use super::{php_loose_string_equal, php_loose_string_value_equal};
    use serde_json::{Value, json};

    // 固定 Zend 源码推导的回归输入；新增 PHP oracle 仍待独立执行，不冒充已运行。
    #[test]
    fn string_value_membership_retains_boolean_null_numeric_and_composite_categories() {
        for (needle, value, expected) in [
            ("01", json!(1), true),
            ("anything", json!(true), true),
            ("0", json!(false), true),
            ("-0", json!(false), false),
            ("-0", json!(0), true),
            ("", json!(false), true),
            ("false", json!(false), false),
            ("", Value::Null, true),
            ("0", Value::Null, false),
            (" ", Value::Null, false),
            ("text", json!(0), false),
            ("", json!(0), false),
            ("0", json!(0), true),
            ("1tail", json!(1), false),
            (" 1 ", json!(1), true),
            ("1.5tail", json!(1.5), false),
            ("0", json!(-0.0), true),
            ("text", json!([]), false),
            ("text", json!({}), false),
            ("01", json!("1"), true),
            ("1tail", json!("1"), false),
        ] {
            assert_eq!(
                php_loose_string_value_equal(needle, &value),
                expected,
                "{needle:?} {value:?}"
            );
        }
    }

    #[test]
    fn number_comparison_does_not_reuse_smart_string_overflow_or_lose_exact_integers() {
        assert!(!php_loose_string_value_equal(
            "9007199254740993",
            &json!(9_007_199_254_740_992i64)
        ));
        assert!(php_loose_string_value_equal(
            "9007199254740993",
            &json!(9_007_199_254_740_992.0)
        ));
        assert!(php_loose_string_value_equal(
            "9223372036854775808",
            &json!(i64::MAX)
        ));
        assert!(!php_loose_string_equal(
            "9223372036854775808",
            "9223372036854775807"
        ));
        assert!(php_loose_string_value_equal(
            "-9223372036854775809",
            &json!(i64::MIN)
        ));
        assert!(php_loose_string_value_equal("1e3", &json!(1000)));
        assert!(php_loose_string_value_equal("1e-9999", &json!(0)));
        assert!(!php_loose_string_value_equal("1e9999", &json!(i64::MAX)));
        let a = format!("0.{}1e20000", "0".repeat(19_999));
        let b = format!("1{}e-20000", "0".repeat(20_000));
        assert!(php_loose_string_value_equal(&a, &json!(0.1)));
        assert!(php_loose_string_value_equal(&b, &json!(10)));
        assert!(!php_loose_string_equal(&b, "10"));
    }
}

#[cfg(test)]
mod minimum_integer_tests {
    use super::{InputNumber, php_loose_string_equal, php_loose_string_value_equal};
    use serde_json::json;

    // 期望来自固定 Zend 源码；对应 PHP 8.5.12 实际运行结果另行采集。
    #[test]
    fn minimum_integer_trailing_whitespace_changes_the_php_number_category() {
        let minimum = "-9223372036854775808";
        let neighbor = "-9223372036854775807";
        let bits = (-9_223_372_036_854_775_808.0_f64).to_bits();
        for space in [' ', '\t', '\n', '\r', '\u{b}', '\u{c}'] {
            for spelling in [minimum, "-0009223372036854775808"] {
                let leading = format!("{space}{spelling}");
                let trailing = format!("{leading}{space}");
                assert!(matches!(
                    InputNumber::numeric_string(&leading),
                    Some(InputNumber::Integer(i64::MIN))
                ));
                assert!(
                    matches!(InputNumber::numeric_string(&trailing), Some(InputNumber::Float(n)) if n.to_bits() == bits)
                );
                assert!(!php_loose_string_equal(&trailing, minimum));
                assert!(!php_loose_string_equal(minimum, &trailing));
                assert!(php_loose_string_equal(&trailing, &trailing));
                assert!(php_loose_string_value_equal(
                    &trailing,
                    &json!(i64::MIN + 1)
                ));
                assert!(!php_loose_string_value_equal(
                    &leading,
                    &json!(i64::MIN + 1)
                ));
                assert!(php_loose_string_value_equal(&trailing, &json!(i64::MIN)));
            }
            let ordinary = format!("{neighbor}{space}");
            assert!(
                matches!(InputNumber::numeric_string(&ordinary), Some(InputNumber::Integer(n)) if n == i64::MIN + 1)
            );
            assert!(php_loose_string_equal(neighbor, &ordinary));
            assert!(php_loose_string_equal(&ordinary, neighbor));
        }
    }
}
