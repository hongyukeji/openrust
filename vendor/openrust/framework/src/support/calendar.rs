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
mod zone;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
pub(crate) use format::display as format_php_date;
use zone::ResolvedTimezone;

const MAX_CALENDAR_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CalendarError {
    InvalidExpression,
    OutOfRange,
    LimitExceeded,
}
type Result<T> = std::result::Result<T, CalendarError>;

#[derive(Clone, Copy)]
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
