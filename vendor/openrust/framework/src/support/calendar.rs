//! PHP date-expression parsing with an explicit reference clock and time zone.

mod abbreviations;
mod civil;
mod format;
pub(crate) mod identifiers;
mod interval;
mod lexer;
mod parse;
mod posix;
mod relative;
mod state;
mod tail_rules;
mod timestamp;
mod zone;

pub(crate) use format::display as format_php_date;
pub use interval::{InputInterval, IntervalError};
use jiff::{Timestamp, tz::TimeZone};
use serde_json::Value;
use zone::ResolvedTimezone;

const MAX_CALENDAR_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CalendarError {
    InvalidExpression,
    OutOfRange,
    LimitExceeded,
}
type Result<T> = std::result::Result<T, CalendarError>;

#[derive(Clone, Copy, Debug)]
struct CalendarInstant {
    epoch_seconds: i64,
    microseconds: u32,
}

impl CalendarInstant {
    fn new(epoch_seconds: i64, microseconds: u32) -> Result<Self> {
        if microseconds >= 1_000_000 {
            return Err(range_error());
        }
        Ok(Self {
            epoch_seconds,
            microseconds,
        })
    }

    fn from_timestamp(value: Timestamp) -> Result<Self> {
        let nanos = value.as_nanosecond();
        Self::new(
            i64::try_from(nanos.div_euclid(1_000_000_000)).map_err(|_| range_error())?,
            u32::try_from(nanos.rem_euclid(1_000_000_000) / 1000).map_err(|_| range_error())?,
        )
    }

    fn try_timestamp(self) -> Result<Timestamp> {
        Timestamp::new(
            self.epoch_seconds,
            i32::try_from(self.microseconds * 1000).map_err(|_| range_error())?,
        )
        .map_err(|_| range_error())
    }
}

/// A parsed request date with its original time zone and microsecond precision.
#[derive(Clone, Debug)]
pub struct InputDate {
    instant: CalendarInstant,
    zone: ResolvedTimezone,
}

impl InputDate {
    fn new(instant: CalendarInstant, zone: ResolvedTimezone) -> Self {
        Self { instant, zone }
    }

    pub(crate) fn native_zoned(&self) -> std::result::Result<jiff::Zoned, super::DateError> {
        let timestamp = self
            .instant
            .try_timestamp()
            .map_err(|_| super::DateError::OutOfRange)?;
        let zone = self.zone.jiff_zone().map_or_else(
            || {
                super::Date::timezone(self.timezone()).or_else(|_| {
                    let offset = self
                        .zone
                        .offset_seconds(self.instant)
                        .map_err(|_| super::DateError::OutOfRange)?;
                    Ok(jiff::tz::TimeZone::fixed(
                        jiff::tz::Offset::from_seconds(offset)
                            .map_err(|_| super::DateError::OutOfRange)?,
                    ))
                })
            },
            Ok,
        )?;
        Ok(timestamp.to_zoned(zone))
    }
    pub(crate) fn format_at(
        &self,
        timestamp: Timestamp,
        pattern: &str,
    ) -> std::result::Result<String, super::DateError> {
        let instant =
            CalendarInstant::from_timestamp(timestamp).map_err(|_| super::DateError::OutOfRange)?;
        format::display_zoned(instant, &self.zone, pattern)
            .map_err(|_| super::DateError::OutOfRange)
    }
    pub fn timestamp(&self) -> i64 {
        self.instant.epoch_seconds
    }

    pub fn microseconds(&self) -> u32 {
        self.instant.microseconds
    }

    pub fn timezone(&self) -> &str {
        self.zone.name()
    }

    pub fn format(&self, pattern: &str) -> std::result::Result<String, super::DateError> {
        format::display_zoned(self.instant, &self.zone, pattern)
            .map_err(|_| super::DateError::OutOfRange)
    }
}

pub(crate) fn format_date_value(
    timestamp: Timestamp,
    timezone: &TimeZone,
    pattern: &str,
) -> std::result::Result<String, super::DateError> {
    let instant =
        CalendarInstant::from_timestamp(timestamp).map_err(|_| super::DateError::OutOfRange)?;
    let zone = ResolvedTimezone::from_jiff(timezone.clone(), timestamp);
    format::display_zoned(instant, &zone, pattern).map_err(|_| super::DateError::OutOfRange)
}

pub(crate) enum DateReference {
    Wall(Timestamp),
    Frozen(Timestamp),
}

pub(crate) fn parse_input_date(
    value: Value,
    pattern: Option<&str>,
    timezone: &str,
    reference: DateReference,
) -> Result<InputDate> {
    let size = value
        .as_str()
        .map_or(0, str::len)
        .saturating_add(pattern.map_or(0, str::len))
        .saturating_add(timezone.len());
    if size > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    parse_input_date_with_zone(
        value,
        pattern,
        ResolvedTimezone::parse(timezone)?,
        reference,
    )
}

pub(crate) fn parse_date_with_jiff(
    value: Value,
    timezone: TimeZone,
    reference: DateReference,
) -> Result<InputDate> {
    if value.as_str().is_some_and(|v| v.len() > MAX_CALENDAR_BYTES) {
        return Err(limit_error());
    }
    let now = match &reference {
        DateReference::Wall(now) | DateReference::Frozen(now) => *now,
    };
    parse_input_date_with_zone(
        value,
        None,
        ResolvedTimezone::from_jiff(timezone, now),
        reference,
    )
}

pub(crate) fn date_from_timestamp(value: Value, milliseconds: bool) -> Result<InputDate> {
    let text = match value {
        Value::String(text) => text,
        Value::Number(number) => match number.as_i64() {
            Some(value) => value.to_string(),
            None => timestamp::number_format_timestamp_precision(
                number.as_f64().ok_or_else(range_error)?,
                if milliseconds { 3 } else { 6 },
            )?,
        },
        _ => return Err(CalendarError::InvalidExpression),
    };
    if text.len() > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    let instant = if milliseconds {
        timestamp::carbon_timestamp_milliseconds(&text)?
    } else {
        timestamp::carbon_timestamp(&text)?
    };
    Ok(InputDate::new(instant, ResolvedTimezone::parse("+00:00")?))
}

pub(crate) fn native_timezone_name(timezone: &TimeZone, timestamp: Timestamp) -> String {
    ResolvedTimezone::from_jiff(timezone.clone(), timestamp)
        .name()
        .to_owned()
}

fn parse_input_date_with_zone(
    value: Value,
    pattern: Option<&str>,
    zone: ResolvedTimezone,
    reference: DateReference,
) -> Result<InputDate> {
    let (reference, frozen) = match reference {
        DateReference::Wall(reference) => (reference, false),
        DateReference::Frozen(reference) => (reference, true),
    };
    let now = CalendarInstant::from_timestamp(reference)?;

    if let Some(pattern) = pattern {
        if value.is_array() || value.is_object() {
            return Err(CalendarError::InvalidExpression);
        }
        let text = super::input_scalars::string(value);
        let mut civil = zone.civil(now)?;
        if !frozen {
            civil.microsecond = 0;
        }
        return format::parse(&text, pattern, civil, &zone);
    }
    let utc = || ResolvedTimezone::parse("+00:00");
    let timestamp = |text| Ok(InputDate::new(timestamp::carbon_timestamp(text)?, utc()?));
    match value {
        Value::Bool(value) => Ok(InputDate::new(
            CalendarInstant::new(i64::from(value), 0)?,
            utc()?,
        )),
        Value::Number(number) => {
            let text = match number.as_i64() {
                Some(integer) => integer.to_string(),
                None => {
                    timestamp::number_format_timestamp(number.as_f64().ok_or_else(range_error)?)?
                }
            };
            timestamp(&text)
        }
        Value::String(text) => {
            let numeric = text.trim_matches(php_whitespace);
            let short_digits = text.len() <= 14 && text.bytes().all(|byte| byte.is_ascii_digit());
            if let Some(epoch) = text.strip_prefix('@')
                && let Ok(parsed) = timestamp(epoch)
            {
                return Ok(parsed);
            }
            if !short_digits
                && is_numeric(numeric)
                && let Ok(parsed) = timestamp(&text)
            {
                return Ok(parsed);
            }
            parse::parse_date(&text, now, &zone, frozen)
        }
        _ => Err(CalendarError::InvalidExpression),
    }
}

pub(crate) fn parse_timestamp_expression(
    expression: &str,
    reference: Timestamp,
    timezone: TimeZone,
) -> Result<i64> {
    Ok(parse_expression(expression, reference, timezone)?.epoch_seconds)
}

fn parse_expression(
    expression: &str,
    reference: Timestamp,
    timezone: TimeZone,
) -> Result<CalendarInstant> {
    if expression.len() > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    if expression.trim_matches(php_whitespace).is_empty() {
        return Err(calendar_error("input_date_invalid"));
    }
    let reference = CalendarInstant::from_timestamp(reference)?;
    let zone = resolved_timezone(&timezone)?;
    parse::parse_text(expression, reference, &zone)
}

fn resolved_timezone(timezone: &TimeZone) -> Result<ResolvedTimezone> {
    match timezone.iana_name() {
        Some(name) => ResolvedTimezone::parse(name),
        None => ResolvedTimezone::parse(&zone::offset_name(
            timezone.to_offset(Timestamp::UNIX_EPOCH).seconds(),
        )),
    }
}

pub(crate) fn php_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
}

pub(crate) fn date_interval(
    date: &str,
    interval: &str,
    reference: Timestamp,
    timezone: TimeZone,
) -> Result<(Timestamp, Timestamp)> {
    if date.len().saturating_add(interval.len()) > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    if interval.trim_ascii().is_empty() {
        return Err(calendar_error("input_date_invalid"));
    }
    let reference = CalendarInstant::from_timestamp(reference)?;
    let zone = resolved_timezone(&timezone)?;
    let (start, end) = parse::add_interval(date, interval, reference, &zone)?;
    Ok((start.try_timestamp()?, end.try_timestamp()?))
}

pub(crate) fn is_numeric(expression: &str) -> bool {
    lexer::is_numeric(expression)
}

fn calendar_error(code: &'static str) -> CalendarError {
    match code {
        "input_calendar_out_of_range" | "input_calendar_timezone_range_unsupported" => {
            CalendarError::OutOfRange
        }
        "input_limits_exceeded" => CalendarError::LimitExceeded,
        _ => CalendarError::InvalidExpression,
    }
}

fn range_error() -> CalendarError {
    calendar_error("input_calendar_out_of_range")
}

fn limit_error() -> CalendarError {
    calendar_error("input_limits_exceeded")
}

#[cfg(test)]
#[path = "../../tests/unit/calendar_intervals.rs"]
mod tests;

pub(crate) fn valid_date(value: &str) -> bool {
    parse::valid_date(value)
}
