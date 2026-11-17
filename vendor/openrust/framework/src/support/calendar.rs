//! PHP date-expression parsing with an explicit reference clock and time zone.

mod abbreviations;
mod civil;
mod format;
mod lexer;
mod parse;
mod posix;
mod relative;
mod state;
mod tail_rules;
mod timestamp;
mod zone;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
pub(crate) use format::display as format_php_date;
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

    fn try_chrono(self) -> Result<DateTime<Utc>> {
        DateTime::from_timestamp(self.epoch_seconds, self.microseconds * 1000)
            .ok_or_else(range_error)
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

pub(crate) fn parse_input_date(
    value: Value,
    pattern: Option<&str>,
    timezone: &str,
    reference: DateTime<Utc>,
) -> Result<InputDate> {
    let size = value
        .as_str()
        .map_or(0, str::len)
        .saturating_add(pattern.map_or(0, str::len))
        .saturating_add(timezone.len());
    if size > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    let now = CalendarInstant::new(reference.timestamp(), reference.timestamp_subsec_micros())?;
    let zone = ResolvedTimezone::parse(timezone)?;
    if let Some(pattern) = pattern {
        if value.is_array() || value.is_object() {
            return Err(CalendarError::InvalidExpression);
        }
        let text = super::input_scalars::string(value);
        let mut civil = zone.civil(now)?;
        civil.microsecond = 0;
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
            parse::parse_date(&text, now, &zone)
        }
        _ => Err(CalendarError::InvalidExpression),
    }
}

pub(crate) fn parse_timestamp_expression(
    expression: &str,
    reference: DateTime<Utc>,
    timezone: Tz,
) -> Result<i64> {
    Ok(parse_expression(expression, reference, timezone)?.epoch_seconds)
}

fn parse_expression(
    expression: &str,
    reference: DateTime<Utc>,
    timezone: Tz,
) -> Result<CalendarInstant> {
    if expression.len() > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    if expression.trim_matches(php_whitespace).is_empty() {
        return Err(calendar_error("input_date_invalid"));
    }
    let reference =
        CalendarInstant::new(reference.timestamp(), reference.timestamp_subsec_micros())?;
    let zone = ResolvedTimezone::parse(timezone.name())?;
    parse::parse_text(expression, reference, &zone)
}

pub(crate) fn php_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
}

pub(crate) fn date_interval(
    date: &str,
    interval: &str,
    reference: DateTime<Utc>,
    timezone: Tz,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    if date.len().saturating_add(interval.len()) > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    if interval.trim_ascii().is_empty() {
        return Err(calendar_error("input_date_invalid"));
    }
    let reference =
        CalendarInstant::new(reference.timestamp(), reference.timestamp_subsec_micros())?;
    let zone = ResolvedTimezone::parse(timezone.name())?;
    let (start, end) = parse::add_interval(date, interval, reference, &zone)?;
    Ok((start.try_chrono()?, end.try_chrono()?))
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
