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
use zone::ResolvedTimezone;

const MAX_CALENDAR_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CalendarError;
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

pub(crate) fn parse_date_expression(
    expression: &str,
    reference: DateTime<Utc>,
    timezone: Tz,
) -> Result<DateTime<Utc>> {
    if expression.is_empty() {
        return Err(calendar_error("input_date_invalid"));
    }
    if expression.len() > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    let reference =
        CalendarInstant::new(reference.timestamp(), reference.timestamp_subsec_micros())?;
    let zone = ResolvedTimezone::parse(timezone.name())?;
    parse::parse_text(expression, reference, &zone)?.try_chrono()
}

pub(crate) fn is_numeric(expression: &str) -> bool {
    lexer::is_numeric(expression)
}

fn calendar_error(_code: &'static str) -> CalendarError {
    CalendarError
}

fn range_error() -> CalendarError {
    calendar_error("input_calendar_out_of_range")
}

fn limit_error() -> CalendarError {
    calendar_error("input_limits_exceeded")
}
