//! PostgreSQL NUMERIC 的纯 wire 解码，不依赖数据库驱动或连接。

use super::{Result, invalid};
use std::fmt::Write;

/// NUMERIC wire 的 ndigits/weight/sign/dscale 与每组 0..9999 按网络序解释。
pub(super) fn numeric(payload: &[u8]) -> Result<String> {
    if payload.len() < 8 || !payload.len().is_multiple_of(2) {
        return Err(invalid());
    }
    let word = |offset: usize| u16::from_be_bytes([payload[offset], payload[offset + 1]]);
    let count = usize::from(word(0));
    if payload.len() != 8 + count * 2 {
        return Err(invalid());
    }
    let weight = i16::from_be_bytes([payload[2], payload[3]]);
    let sign = word(4);
    let scale = usize::from(word(6));
    if scale > 0x3fff {
        return Err(invalid());
    }
    if matches!(sign, 0xc000 | 0xd000 | 0xf000) {
        return if count == 0 {
            Ok(match sign {
                0xc000 => "NaN",
                0xd000 => "Infinity",
                _ => "-Infinity",
            }
            .to_owned())
        } else {
            Err(invalid())
        };
    }
    if !matches!(sign, 0x0000 | 0x4000) {
        return Err(invalid());
    }
    let mut digits = Vec::with_capacity(count);
    for index in 0..count {
        let digit = word(8 + index * 2);
        if digit >= 10_000 {
            return Err(invalid());
        }
        let power = i32::from(weight) - i32::try_from(index).map_err(|_| invalid())?;
        let hidden = (-power * 4 - i32::try_from(scale).map_err(|_| invalid())?).clamp(0, 4);
        if !digit.is_multiple_of(10_u16.pow(u32::try_from(hidden).map_err(|_| invalid())?)) {
            return Err(invalid());
        }
        digits.push(digit);
    }
    let mut output = String::new();
    if sign == 0x4000 && digits.iter().any(|digit| *digit != 0) {
        output.push('-');
    }
    if weight < 0 || digits.is_empty() {
        output.push('0');
    } else {
        let integer_groups = usize::try_from(i32::from(weight) + 1).map_err(|_| invalid())?;
        for index in 0..integer_groups {
            let digit = digits.get(index).copied().unwrap_or(0);
            if index == 0 {
                write!(output, "{digit}").map_err(|_| invalid())?;
            } else {
                write!(output, "{digit:04}").map_err(|_| invalid())?;
            }
        }
    }
    if scale != 0 {
        output.push('.');
        let start = output.len();
        for fraction in 0..scale.div_ceil(4) {
            let index = i32::from(weight) + 1 + i32::try_from(fraction).map_err(|_| invalid())?;
            let digit = usize::try_from(index)
                .ok()
                .and_then(|index| digits.get(index))
                .copied()
                .unwrap_or(0);
            write!(output, "{digit:04}").map_err(|_| invalid())?;
        }
        output.truncate(start + scale);
    }
    Ok(output)
}
