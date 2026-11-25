//! Carbon numeric timestamp parsing. See SOURCES.md and LICENSE.

use super::{CalendarInstant, Result, range_error};

/// Carbon Timestamp 的字符串 chunks 相加；首符号统辖全部数字，不把 e 当指数。
pub(super) fn carbon_timestamp(text: &str) -> Result<CalendarInstant> {
    let (negative, seconds, micros) = parts(text, 1_000_000.0)?;
    // Carbon 的 floor 中间值为 float，precision14 将该量级转为指数，U u 格式因此失败。
    if seconds
        .checked_add(i128::from(micros / 1_000_000))
        .ok_or_else(range_error)?
        >= 100_000_000_000_000
    {
        return Err(range_error());
    }
    let total = seconds
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_add(i128::from(micros)))
        .ok_or_else(range_error)?;
    let total = if negative {
        total.checked_neg().ok_or_else(range_error)?
    } else {
        total
    };
    CalendarInstant::new(
        i64::try_from(total.div_euclid(1_000_000)).map_err(|_| range_error())?,
        u32::try_from(total.rem_euclid(1_000_000)).map_err(|_| range_error())?,
    )
}

fn parts(text: &str, scale: f64) -> Result<(bool, i128, i64)> {
    let negative = text.starts_with('-');
    let mut seconds = 0i128;
    let mut fractional = 0f64;
    for chunk in text.split(|c: char| !c.is_ascii_digit() && c != '.') {
        let mut parts = chunk.split('.');
        let integer = parts.next().unwrap_or("");
        if !integer.is_empty() {
            seconds = seconds
                .checked_add(integer.parse::<i128>().map_err(|_| range_error())?)
                .ok_or_else(range_error)?;
        }
        let decimal = parts.next().unwrap_or("");
        if !decimal.is_empty() {
            fractional += format!("0.{decimal}")
                .parse::<f64>()
                .map_err(|_| range_error())?;
        }
    }
    // 与固定 Carbon round(..., 3/6) 一致，限制转换区间而不截断 i64 epoch。
    let carry = fractional.floor();
    if !carry.is_finite() || carry > 20_971_520.0 {
        return Err(range_error());
    }
    #[allow(clippy::cast_possible_truncation)]
    let carry = carry as i32;
    seconds = seconds
        .checked_add(i128::from(carry))
        .ok_or_else(range_error)?;
    let fraction = ((fractional - f64::from(carry)) * scale).round();
    #[allow(clippy::cast_possible_truncation)]
    let micros = fraction as i64;
    Ok((negative, seconds, micros))
}

/// 固定上游先拆分毫秒整数与小数，再组合秒和微秒；负数不可直接除以1000替代。
pub(super) fn carbon_timestamp_milliseconds(text: &str) -> Result<CalendarInstant> {
    let (negative, milliseconds, microseconds) = parts(text, 1000.0)?;
    let sign = if negative { -1i128 } else { 1i128 };
    let micros = sign * i128::from(microseconds) + (milliseconds % 1000) * 1000;
    let seconds = sign * (milliseconds / 1000) + micros.div_euclid(1_000_000);
    CalendarInstant::new(
        i64::try_from(seconds).map_err(|_| range_error())?,
        u32::try_from(micros.rem_euclid(1_000_000)).map_err(|_| range_error())?,
    )
}

/// 固定 PHP 8.5 `number_format` 的6位 half-up；不是 Rust format 的ties-even。
/// Copyright (c) The PHP Group；移植 ext/standard/math.c，完整 PHP3.01许可见LICENSE。
pub(super) fn number_format_timestamp(value: f64) -> Result<String> {
    number_format_timestamp_precision(value, 6)
}

#[allow(clippy::float_cmp)]
pub(super) fn number_format_timestamp_precision(value: f64, precision: usize) -> Result<String> {
    if !value.is_finite() {
        return Err(range_error());
    }
    let exponent = match precision {
        2 => 100.0,
        3 => 1000.0,
        6 => 1_000_000.0,
        _ => return Err(range_error()),
    };
    let mut integral = (value * exponent).trunc();
    let adjacent = integral + 1.0f64.copysign(value);
    if adjacent / exponent == value {
        integral = adjacent;
    }
    let rounded = if integral.abs() >= 1e16 {
        value
    } else {
        let edge = ((integral + 0.5f64.copysign(value)) / exponent).abs();
        if value.abs() >= edge {
            integral += 1.0f64.copysign(value);
        }
        integral / exponent
    };
    // PHP number_format 将负零及舍入到零的负数输出为无符号零。
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    Ok(format!("{rounded:.precision$}"))
}
