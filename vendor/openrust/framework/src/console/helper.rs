// 许可见 LICENSE.symfony；文本机制复用既有字素与格式器。
mod width_tables;
use super::{ConsoleError, ConsoleFormatError, OutputFormatterInterface, formatting::unicode};
use crate::support::{NumberValue, input_scalars};
use std::collections::BTreeMap;
/// 通用帮助器的文本与数值工具；关联集合入口单独维护。
#[non_exhaustive]
pub struct Helper;
fn contains(ranges: &[(u32, u32)], value: u32) -> bool {
    let at = ranges.partition_point(|(_, end)| *end < value);
    ranges.get(at).is_some_and(|(start, _)| *start <= value)
}
impl Helper {
    pub fn width(value: Option<&str>) -> usize {
        let text = value.unwrap_or_default();
        let mut clean = String::with_capacity(text.len());
        let mut groups = 0;
        let mut previous = false;
        for ch in text.chars() {
            if ch.is_control() {
                if !previous {
                    groups += 1;
                }
                previous = true;
            } else {
                clean.push(ch);
                previous = false;
            }
        }
        let normalized = icu_normalizer::ComposingNormalizer::new_nfc().normalize(&clean);
        groups
            + normalized
                .chars()
                .map(|ch| {
                    let cp = u32::from(ch);
                    if cp == 0
                        || cp == 0x034f
                        || (0x200b..=0x200f).contains(&cp)
                        || matches!(cp, 0x2028 | 0x2029)
                        || (0x202a..=0x202e).contains(&cp)
                        || (0x2060..=0x2063).contains(&cp)
                        || contains(width_tables::ZERO, cp)
                    {
                        0
                    } else if contains(width_tables::WIDE, cp) {
                        2
                    } else {
                        1
                    }
                })
                .sum::<usize>()
    }
    pub fn length(value: Option<&str>) -> usize {
        unicode::length(value.unwrap_or_default())
    }
    pub fn substr(value: Option<&str>, from: i64, length: Option<i64>) -> String {
        let text = value.unwrap_or_default();
        let count = unicode::length(text);
        let (start, end) = bounds(count, from, length.or(Some(2147483647)));
        if start >= end {
            return String::new();
        }
        unicode::Slices::new(text)
            .slice(start, Some(end))
            .to_owned()
    }
    pub fn width_bytes(value: Option<&[u8]>) -> usize {
        let bytes = value.unwrap_or_default();
        std::str::from_utf8(bytes).map_or(bytes.len(), |v| Self::width(Some(v)))
    }
    pub fn length_bytes(value: Option<&[u8]>) -> usize {
        let bytes = value.unwrap_or_default();
        match std::str::from_utf8(bytes) {
            Ok(v) => Self::length(Some(v)),
            Err(_) => bytes.len(),
        }
    }
    pub fn substr_bytes(value: Option<&[u8]>, from: i64, length: Option<i64>) -> Vec<u8> {
        let bytes = value.unwrap_or_default();
        match std::str::from_utf8(bytes) {
            Ok(v) => Self::substr(Some(v), from, length).into_bytes(),
            Err(_) => {
                let (a, z) = bounds(bytes.len(), from, length);
                bytes[a..z].to_vec()
            }
        }
    }
    pub fn format_time(seconds: impl Into<NumberValue>) -> Result<String, ConsoleError> {
        Self::format_time_with(seconds, 1)
    }
    pub fn format_time_with(
        seconds: impl Into<NumberValue>,
        precision: i64,
    ) -> Result<String, ConsoleError> {
        let mut ms = match seconds.into() {
            NumberValue::Integer(i) => match i.checked_mul(1000) {
                Some(ms) => ms,
                None => milliseconds(i as f64 * 1000.0)?,
            },
            NumberValue::Float(f) => milliseconds(f * 1000.0)?,
        };
        if ms == 0 {
            return Ok("< 1 ms".into());
        }
        let formats = [
            (1, "ms"),
            (1000, "s"),
            (60000, "min"),
            (3600000, "h"),
            (86400000, "d"),
        ];
        let mut values = BTreeMap::new();
        for (index, (unit, label)) in formats.iter().enumerate() {
            let part = formats.get(index + 1).map_or(ms, |(next, _)| ms % next);
            if let Ok(old) = usize::try_from(index as i128 - i128::from(precision)) {
                values.remove(&old);
            }
            if part == 0 {
                continue;
            }
            values.insert(
                index,
                format!(
                    "{} {label}",
                    input_scalars::double_string(part as f64 / *unit as f64)
                ),
            );
            if ms == part {
                break;
            }
            ms -= part;
        }
        Ok(values.into_values().rev().collect::<Vec<_>>().join(", "))
    }
    pub fn format_memory(memory: i64) -> String {
        if memory >= 1 << 30 {
            format!("{:.1} GiB", memory as f64 / (1u64 << 30) as f64)
        } else if memory >= 1 << 20 {
            format!("{:.1} MiB", memory as f64 / (1u64 << 20) as f64)
        } else if memory >= 1 << 10 {
            format!("{} KiB", memory / 1024)
        } else {
            format!("{memory} B")
        }
    }
    pub fn remove_decoration(
        formatter: &dyn OutputFormatterInterface,
        text: Option<&str>,
    ) -> Result<String, ConsoleFormatError> {
        let previous = formatter.is_decorated();
        formatter.set_decorated(false);
        let plain = formatter
            .format(Some(text.unwrap_or_default()))?
            .unwrap_or_default();
        let plain = super::formatting::strip_decoration(plain);
        formatter.set_decorated(previous);
        Ok(plain)
    }
}
fn milliseconds(value: f64) -> Result<i64, ConsoleError> {
    if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
        return Err(ConsoleError::new("时间辅助值超出整数范围"));
    }
    Ok(input_scalars::double_integer(value))
}
fn bounds(count: usize, from: i64, length: Option<i64>) -> (usize, usize) {
    let count = count as i128;
    let start = if from < 0 {
        (count + i128::from(from)).max(0)
    } else {
        i128::from(from).min(count)
    };
    let end = match length {
        None => count,
        Some(n) if n >= 0 => (start + i128::from(n)).min(count),
        Some(n) => (count + i128::from(n)).max(start),
    };
    (start as usize, end as usize)
}
macro_rules! impl_helper_tools {
    ($ty:ty) => {
        impl $ty {
            pub fn width(value: Option<&str>) -> usize {
                $crate::console::Helper::width(value)
            }
            pub fn length(value: Option<&str>) -> usize {
                $crate::console::Helper::length(value)
            }
            pub fn substr(value: Option<&str>, from: i64, length: Option<i64>) -> String {
                $crate::console::Helper::substr(value, from, length)
            }
            pub fn format_time(
                seconds: impl Into<$crate::support::NumberValue>,
            ) -> Result<String, $crate::console::ConsoleError> {
                $crate::console::Helper::format_time(seconds)
            }
            pub fn format_time_with(
                seconds: impl Into<$crate::support::NumberValue>,
                precision: i64,
            ) -> Result<String, $crate::console::ConsoleError> {
                $crate::console::Helper::format_time_with(seconds, precision)
            }
            pub fn format_memory(memory: i64) -> String {
                $crate::console::Helper::format_memory(memory)
            }
            pub fn remove_decoration(
                formatter: &dyn $crate::console::OutputFormatterInterface,
                text: Option<&str>,
            ) -> Result<String, $crate::console::ConsoleFormatError> {
                $crate::console::Helper::remove_decoration(formatter, text)
            }
        }
    };
}
pub(super) use impl_helper_tools;
