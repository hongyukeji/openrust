use bigdecimal::{BigDecimal, RoundingMode, Zero, num_bigint::BigInt};

use crate::database::{DatabaseError, DatabaseResult};

const MAX_TEXT: usize = 16_384;
const MAX_SCALE: i64 = 16_384;

pub(super) fn read(text: &str, scale: u32) -> DatabaseResult<String> {
    let scale = i64::from(scale);
    if text.len() > MAX_TEXT || scale > MAX_SCALE {
        return Err(limit_exceeded());
    }
    let number = parse(text)?;
    let digits = i64::try_from(number.digits()).map_err(|_| limit_exceeded())?;
    let integer_digits = if number.is_zero() {
        1
    } else {
        (digits - number.fractional_digit_count()).max(1)
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
    Ok(output)
}

fn parse(text: &str) -> DatabaseResult<BigDecimal> {
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

fn parse_exponent(exponent: &str) -> DatabaseResult<i64> {
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

fn unsigned_integer(digits: &str) -> DatabaseResult<BigInt> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(cast_failed());
    }
    BigInt::parse_bytes(digits.as_bytes(), 10).ok_or_else(cast_failed)
}

fn parse_rational(
    numerator: &str,
    denominator: &str,
    negative: bool,
) -> DatabaseResult<BigDecimal> {
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

fn cast_failed() -> DatabaseError {
    DatabaseError::InvalidQuery("decimal attribute")
}

fn limit_exceeded() -> DatabaseError {
    DatabaseError::InvalidQuery("decimal attribute size")
}
