//! 显式时区身份、历史 transition 与 RFC 8536 POSIX 尾规则。
//! 固定来源与 MIT 许可见同目录 SOURCES.md / LICENSE.timelib。

use super::civil::Civil;
use super::posix::{self, Tail, TailRecord};
use super::{CalendarInstant, calendar_error};
use crate::Result;
use chrono::{LocalResult, Offset, TimeZone};
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
enum ZoneKind {
    Fixed(i32),
    Named {
        zone: Option<chrono_tz::Tz>,
        record: &'static TailRecord,
        tail: Tail,
    },
}

/// 已解析的 IANA、固定偏移或缩写时区；保留原始时区身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTimezone {
    name: String,
    kind: ZoneKind,
}
impl Default for ResolvedTimezone {
    fn default() -> Self {
        Self::utc()
    }
}
impl ResolvedTimezone {
    /// 构造 UTC 时区。
    pub fn utc() -> Self {
        Self {
            name: "UTC".into(),
            kind: ZoneKind::Fixed(0),
        }
    }
    /// 解析固定 IANA 2025b 名称、固定偏移和 PHP 支持的时区缩写。
    pub fn parse(name: &str) -> Result<Self> {
        if name.len() > super::MAX_CALENDAR_BYTES {
            return Err(super::limit_error());
        }
        if name.eq_ignore_ascii_case("UTC") || name.eq_ignore_ascii_case("GMT") {
            return Ok(Self {
                name: name.to_ascii_uppercase(),
                kind: ZoneKind::Fixed(0),
            });
        }
        if let Some(seconds) = parse_offset(name) {
            return Ok(Self {
                name: offset_name(seconds),
                kind: ZoneKind::Fixed(seconds),
            });
        }
        if let Some(record) = posix::record(name) {
            let tail = Tail::parse(record.rule)?;
            return Ok(Self {
                name: record.name.into(),
                kind: ZoneKind::Named {
                    zone: chrono_tz::Tz::from_str(record.name).ok(),
                    record,
                    tail,
                },
            });
        }
        if let Some((_, seconds)) = super::abbreviations::ABBREVIATIONS
            .iter()
            .find(|(key, _)| name.eq_ignore_ascii_case(key))
        {
            return Ok(Self {
                name: name.to_ascii_uppercase(),
                kind: ZoneKind::Fixed(*seconds),
            });
        }
        Err(calendar_error("input_timezone_invalid"))
    }
    /// 按小时偏移构造时区；对应 Carbon 的整数时区参数。
    pub fn from_offset_hours(hours: i64) -> Result<Self> {
        if !(-99..=99).contains(&hours) {
            return Err(calendar_error("input_timezone_invalid"));
        }
        let seconds = hours
            .checked_mul(3600)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| calendar_error("input_timezone_invalid"))?;
        Ok(Self {
            name: offset_name(seconds),
            kind: ZoneKind::Fixed(seconds),
        })
    }
    /// 返回时区名称；固定偏移、缩写与 IANA 身份不会合并。
    pub fn name(&self) -> &str {
        &self.name
    }
    /// 查询指定时刻的 UTC 偏移秒数，包含历史 LMT 和无限期 POSIX 尾规则。
    pub fn offset_seconds(&self, instant: CalendarInstant) -> Result<i32> {
        match &self.kind {
            ZoneKind::Fixed(offset) => Ok(*offset),
            ZoneKind::Named { zone, record, tail } => {
                if instant.epoch_seconds >= record.from_epoch {
                    return Ok(tail.state(instant)?.offset);
                }
                if record
                    .first_epoch
                    .is_some_and(|first| instant.epoch_seconds < first)
                {
                    return Ok(record.first_offset);
                }
                let zone = zone.ok_or_else(|| calendar_error("input_timezone_invalid"))?;
                let utc = instant.try_chrono()?;
                Ok(zone
                    .offset_from_utc_datetime(&utc.naive_utc())
                    .fix()
                    .local_minus_utc())
            }
        }
    }
    pub(super) fn display_name(&self) -> String {
        match self.kind {
            ZoneKind::Fixed(offset) if self.name.starts_with(['+', '-']) => display_offset(offset),
            _ => self.name.clone(),
        }
    }
    pub(super) fn abbreviation(&self, instant: CalendarInstant) -> Result<String> {
        use chrono_tz::OffsetName;
        match &self.kind {
            ZoneKind::Fixed(offset) => {
                if self.name.starts_with(['+', '-']) {
                    let absolute = i64::from(*offset).abs();
                    Ok(format!(
                        "GMT{}{:02}{:02}",
                        if *offset < 0 { '-' } else { '+' },
                        absolute / 3600,
                        absolute / 60 % 60
                    ))
                } else {
                    Ok(self.name.clone())
                }
            }
            ZoneKind::Named { zone, record, tail } => {
                if instant.epoch_seconds >= record.from_epoch {
                    return Ok(tail.state(instant)?.abbreviation.into());
                }
                if record
                    .first_epoch
                    .is_some_and(|first| instant.epoch_seconds < first)
                {
                    return Ok(record.first_abbreviation.into());
                }
                let zone = zone.ok_or_else(|| calendar_error("input_timezone_invalid"))?;
                let utc = instant.try_chrono()?;
                let offset = zone.offset_from_utc_datetime(&utc.naive_utc());
                Ok(offset
                    .abbreviation()
                    .map_or_else(|| self.name.clone(), str::to_owned))
            }
        }
    }
    pub(super) fn is_dst(&self, instant: CalendarInstant) -> Result<bool> {
        use chrono_tz::OffsetComponents;
        match &self.kind {
            ZoneKind::Fixed(_) => Ok(super::abbreviations::DAYLIGHT_ABBREVIATIONS
                .iter()
                .any(|name| self.name.eq_ignore_ascii_case(name))),
            ZoneKind::Named { zone, record, tail } => {
                if instant.epoch_seconds >= record.from_epoch {
                    return Ok(tail.state(instant)?.is_dst);
                }
                if record
                    .first_epoch
                    .is_some_and(|first| instant.epoch_seconds < first)
                {
                    return Ok(record.first_is_dst);
                }
                let zone = zone.ok_or_else(|| calendar_error("input_timezone_invalid"))?;
                let utc = instant.try_chrono()?;
                Ok(zone
                    .offset_from_utc_datetime(&utc.naive_utc())
                    .dst_offset()
                    .num_seconds()
                    != 0)
            }
        }
    }
    pub(super) fn civil(&self, instant: CalendarInstant) -> Result<Civil> {
        Civil::from_instant(instant, self.offset_seconds(instant)?)
    }
    pub(super) fn resolve(&self, civil: Civil) -> Result<CalendarInstant> {
        let civil = civil.normalize()?;
        match &self.kind {
            ZoneKind::Fixed(offset) => civil.instant(*offset),
            ZoneKind::Named { zone, record, tail } => {
                let early = civil.instant(record.first_offset)?;
                if record
                    .first_epoch
                    .is_some_and(|first| early.epoch_seconds < first)
                {
                    return Ok(early);
                }
                if let Some(instant) = tail.resolve(civil, record.from_epoch)? {
                    return Ok(instant);
                }
                let zone = zone.ok_or_else(|| calendar_error("input_timezone_invalid"))?;
                let naive = civil.naive()?;
                match zone.from_local_datetime(&naive) {
                    LocalResult::Single(value) => {
                        CalendarInstant::new(value.timestamp(), value.timestamp_subsec_micros())
                    }
                    LocalResult::Ambiguous(first, second) => {
                        // timelib 暂按 UTC 查询本地分量的偏移；东西半球回拨选择因此不同。
                        let guess = self.offset_seconds(civil.instant(0)?)?;
                        let value = if first.offset().fix().local_minus_utc() == guess {
                            first
                        } else {
                            second
                        };
                        CalendarInstant::new(value.timestamp(), value.timestamp_subsec_micros())
                    }
                    LocalResult::None => {
                        let before = chrono_tz::GapInfo::new(&naive, &zone)
                            .and_then(|gap| {
                                gap.begin.map(|(_, offset)| offset.fix().local_minus_utc())
                            })
                            .ok_or_else(|| calendar_error("input_timezone_invalid"))?;
                        civil.instant(before)
                    }
                }
            }
        }
    }
}

fn parse_offset(name: &str) -> Option<i32> {
    let bytes = name.as_bytes();
    let sign = match bytes.first()? {
        b'+' => 1i32,
        b'-' => -1,
        _ => return None,
    };
    let digits = name.get(1..)?;
    let parts: Vec<_> = digits.split(':').collect();
    if parts
        .iter()
        .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let integer = |value: &str| {
        if value.is_empty() {
            Some(0)
        } else {
            value.parse::<i32>().ok()
        }
    };
    let (hours, minutes, seconds) = if parts.len() > 1 {
        if parts.len() > 3 || parts[0].is_empty() {
            return None;
        }
        if parts.len() == 3 && (parts[1].is_empty() || parts[2].is_empty()) {
            // PHP 8.5 parse_zone 的三段空字段兼容行为；始终拒绝嵌入符号。
            (0, 0, integer(parts[0])?)
        } else {
            (
                integer(parts[0])?,
                integer(parts[1])?,
                parts.get(2).map_or(Some(0), |part| integer(part))?,
            )
        }
    } else {
        match digits.len() {
            1 | 2 => (integer(digits)?, 0, 0),
            4 => (integer(digits.get(..2)?)?, integer(digits.get(2..)?)?, 0),
            6 => (
                integer(digits.get(..2)?)?,
                integer(digits.get(2..4)?)?,
                integer(digits.get(4..)?)?,
            ),
            _ => return None,
        }
    };
    if minutes > 99 || seconds > 99 || hours > 99 {
        return None;
    }
    let offset = hours
        .checked_mul(3600)?
        .checked_add(minutes.checked_mul(60)?)?
        .checked_add(seconds)?;
    if offset >= 360_000 {
        return None;
    }
    offset.checked_mul(sign)
}

pub(super) fn display_offset(offset: i32) -> String {
    let abs = i64::from(offset).abs();
    format!(
        "{}{:02}:{:02}",
        if offset < 0 { '-' } else { '+' },
        abs / 3600,
        abs / 60 % 60
    )
}
pub(super) fn offset_name(offset: i32) -> String {
    let displayed = display_offset(offset);
    let seconds = i64::from(offset).abs() % 60;
    if seconds == 0 {
        displayed
    } else {
        format!("{displayed}:{seconds:02}")
    }
}
