//! Eloquent 的十进制定标。先按 Brick Math 语法解析精确值，再进行 `HalfUp` 舍入。

use bigdecimal::{BigDecimal, RoundingMode, Zero, num_bigint::BigInt};
use serde_json::Value;
use std::borrow::Cow;

use crate::{Error, Result, support::InputNumber};

const MAX_TEXT: usize = 16_384;
const MAX_SCALE: i64 = 16_384;
const I64_START: f64 = -9_223_372_036_854_775_808.0;
const I64_END: f64 = 9_223_372_036_854_775_808.0;

pub(super) fn read(value: &Value, scale: Option<&str>) -> Result<Value> {
    let scale = scale
        .ok_or_else(|| Error::custom("model_decimal_scale_missing", "十进制转换缺少小数位数"))?;
    let text = match value {
        Value::String(text) => Cow::Borrowed(text.as_str()),
        Value::Array(_) | Value::Object(_) => return Err(cast_failed()),
        value => Cow::Owned(crate::support::scalars::string(value)),
    };
    if text.len() > MAX_TEXT {
        return Err(limit_exceeded());
    }
    let number = parse(&text)?;
    let scale = parse_scale(scale)?;

    // 在定标分配前限制整数扩展和目标小数位。舍入进位后还会校验实际输出。
    let old_scale = number.fractional_digit_count();
    let digits = i64::try_from(number.digits()).map_err(|_| limit_exceeded())?;
    let integer_digits = if number.is_zero() {
        1
    } else {
        (digits - old_scale).max(1)
    };
    if integer_digits + scale > MAX_SCALE {
        return Err(limit_exceeded());
    }
    let output = number
        .with_scale_round(scale, RoundingMode::HalfUp)
        .to_plain_string();
    if output.len() > MAX_TEXT {
        return Err(limit_exceeded());
    }
    Ok(Value::String(output))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "已检查 PHP64 的整数范围，弱参数按 PHP 规则向零截断"
)]
fn parse_scale(scale: &str) -> Result<i64> {
    if scale.len() > MAX_TEXT {
        return Err(limit_exceeded());
    }
    let scale = match InputNumber::numeric_string(scale).ok_or_else(invalid_scale)? {
        InputNumber::Integer(value) => value,
        InputNumber::Float(value) if (I64_START..I64_END).contains(&value) => value as i64,
        InputNumber::Float(_) => return Err(invalid_scale()),
    };
    if scale < 0 {
        return Err(cast_failed());
    }
    if scale > MAX_SCALE {
        return Err(limit_exceeded());
    }
    Ok(scale)
}

fn parse(text: &str) -> Result<BigDecimal> {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let negative = text.starts_with('-');
    if let Some((numerator, denominator)) = unsigned.split_once('/') {
        return parse_rational(numerator, denominator, negative);
    }

    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, parse_exponent(exponent)?),
        None => (unsigned, 0),
    };
    let (integral, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if (integral.is_empty() && fraction.is_empty())
        || !integral
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(cast_failed());
    }
    let mut coefficient = String::with_capacity(mantissa.len());
    coefficient.push_str(integral);
    coefficient.push_str(fraction);
    let coefficient = unsigned_integer(&coefficient)?;
    let coefficient = if negative { -coefficient } else { coefficient };
    let fraction_digits = i64::try_from(fraction.len()).map_err(|_| limit_exceeded())?;
    Ok(BigDecimal::new(coefficient, fraction_digits - exponent))
}

fn parse_exponent(exponent: &str) -> Result<i64> {
    let digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(cast_failed());
    }
    let value = exponent.parse::<i64>().map_err(|_| limit_exceeded())?;
    if !(-MAX_SCALE..=MAX_SCALE).contains(&value) {
        return Err(limit_exceeded());
    }
    Ok(value)
}

fn unsigned_integer(digits: &str) -> Result<BigInt> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(cast_failed());
    }
    BigInt::parse_bytes(digits.as_bytes(), 10).ok_or_else(cast_failed)
}

fn parse_rational(numerator: &str, denominator: &str, negative: bool) -> Result<BigDecimal> {
    let numerator = unsigned_integer(numerator)?;
    let denominator = unsigned_integer(denominator)?;
    if denominator.is_zero() {
        return Err(cast_failed());
    }
    let gcd = gcd(numerator.clone(), denominator.clone());
    let numerator = numerator / &gcd;
    let denominator = denominator / gcd;

    // 约分后仅含 2 和 5 的分母才有有限十进制展开。
    let twos = denominator.trailing_zeros().ok_or_else(cast_failed)?;
    let mut factors = &denominator >> twos;
    let mut fives = 0_u64;
    while (&factors % 5_u8).is_zero() {
        factors /= 5_u8;
        fives += 1;
        if fives > MAX_TEXT as u64 {
            return Err(limit_exceeded());
        }
    }
    if factors != BigInt::from(1_u8) {
        return Err(cast_failed());
    }
    let scale = twos.max(fives);
    if scale > MAX_TEXT as u64 {
        return Err(limit_exceeded());
    }
    let power = u32::try_from(scale).map_err(|_| limit_exceeded())?;
    let coefficient = numerator * (BigInt::from(10_u8).pow(power) / denominator);
    let coefficient = if negative { -coefficient } else { coefficient };
    let scale = i64::try_from(scale).map_err(|_| limit_exceeded())?;
    Ok(BigDecimal::new(coefficient, scale))
}

fn gcd(mut left: BigInt, mut right: BigInt) -> BigInt {
    while !right.is_zero() {
        let remainder = left % &right;
        left = right;
        right = remainder;
    }
    left
}

fn invalid_scale() -> Error {
    Error::custom("model_decimal_scale_invalid", "十进制转换的小数位数无效")
}

fn cast_failed() -> Error {
    Error::custom("model_decimal_cast_failed", "无法将模型属性转换为十进制")
}

fn limit_exceeded() -> Error {
    Error::custom("model_decimal_limit_exceeded", "模型十进制转换超出处理上限")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn half_up_preserves_precision_and_handles_finite_rationals() -> Result<()> {
        for (input, scale, expected) in [
            (".8", "2", "0.80"),
            ("1.", "2", "1.00"),
            ("-0.000", "2", "0.00"),
            ("-0.001", "2", "0.00"),
            ("9.995", "2", "10.00"),
            ("-9.995", "2", "-10.00"),
            ("-1/8", "2", "-0.13"),
            ("3/6", "2", "0.50"),
            ("0/3", "2", "0.00"),
            ("0.123456789e3", "20", "123.45678900000000000000"),
            (
                "89898989898989898989.898989898989898989895",
                "20",
                "89898989898989898989.89898989898989898990",
            ),
        ] {
            assert_eq!(
                read(&json!(input), Some(scale))?,
                json!(expected),
                "{input}"
            );
        }
        for scale in ["+02", " 2 ", "2.5", "2e0"] {
            assert_eq!(read(&json!(1.005), Some(scale))?, json!("1.01"));
        }
        assert_eq!(read(&json!(true), Some("2"))?, json!("1.00"));
        Ok(())
    }

    #[test]
    fn failures_keep_scale_syntax_value_and_allocation_boundaries_distinct() {
        for (input, scale, code) in [
            ("1", None, "model_decimal_scale_missing"),
            ("1", Some(""), "model_decimal_scale_invalid"),
            ("1", Some("2:3"), "model_decimal_scale_invalid"),
            ("1", Some("-2"), "model_decimal_cast_failed"),
            (" 1", Some("2"), "model_decimal_cast_failed"),
            ("1_000", Some("2"), "model_decimal_cast_failed"),
            ("2/3", Some("2"), "model_decimal_cast_failed"),
            ("1/-8", Some("2"), "model_decimal_cast_failed"),
            ("1/0", Some("2"), "model_decimal_cast_failed"),
            ("1e+", Some("2"), "model_decimal_cast_failed"),
            (
                "1e99999999999999999999999",
                Some("2"),
                "model_decimal_limit_exceeded",
            ),
            ("1e16384", Some("2"), "model_decimal_limit_exceeded"),
            ("1", Some("16384"), "model_decimal_limit_exceeded"),
            ("1", Some("16383"), "model_decimal_limit_exceeded"),
        ] {
            assert_eq!(
                read(&json!(input), scale).err().as_ref().map(Error::code),
                Some(code),
                "{input} / {scale:?}"
            );
        }
    }
}
