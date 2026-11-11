use std::fmt::Write as _;

use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;

use crate::database::{DatabaseError, DatabaseResult, Value};

use super::AttributeCast;

pub(super) fn read(
    cast: AttributeCast,
    raw: &Value,
    timezone: Tz,
    format: &str,
) -> DatabaseResult<Value> {
    if matches!(raw, Value::Null) {
        return Ok(Value::Null);
    }
    let mut date = parse(raw, timezone, format)?;
    if matches!(cast, AttributeCast::Timestamp) {
        return Ok(Value::Integer(date.timestamp()));
    }
    if matches!(cast, AttributeCast::Date | AttributeCast::DateWithFormat(_)) {
        let midnight = date.date_naive().and_hms_opt(0, 0, 0).ok_or_else(invalid)?;
        date = localize(timezone, midnight)?;
    }
    Ok(date
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Micros, true)
        .into())
}

pub(super) fn store(raw: Value, timezone: Tz, format: &str) -> DatabaseResult<Value> {
    if matches!(raw, Value::Null) {
        return Ok(raw);
    }
    format_for_storage(parse(&raw, timezone, format)?, format)
}

pub(super) fn equivalent(
    current: &Value,
    original: &Value,
    timezone: Tz,
    format: &str,
) -> DatabaseResult<bool> {
    Ok(
        format_for_storage(parse(current, timezone, format)?, format)?
            == format_for_storage(parse(original, timezone, format)?, format)?,
    )
}

pub(in crate::database::eloquent) fn format_for_storage(
    date: DateTime<Tz>,
    format: &str,
) -> DatabaseResult<Value> {
    let mut output = String::new();
    write!(&mut output, "{}", date.format(format)).map_err(|_| invalid())?;
    Ok(output.into())
}

fn parse(raw: &Value, timezone: Tz, format: &str) -> DatabaseResult<DateTime<Tz>> {
    match raw {
        Value::Integer(seconds) => from_timestamp(timezone, *seconds, 0),
        Value::Unsigned(seconds) => {
            from_timestamp(timezone, i64::try_from(*seconds).map_err(|_| invalid())?, 0)
        }
        Value::Real(seconds) => from_fractional_timestamp(timezone, *seconds),
        Value::Text(text) => parse_text(text, timezone, format),
        _ => Err(invalid()),
    }
}

fn parse_text(text: &str, timezone: Tz, format: &str) -> DatabaseResult<DateTime<Tz>> {
    if let Ok(seconds) = text.parse::<i64>() {
        return from_timestamp(timezone, seconds, 0);
    }
    if let Ok(seconds) = text.parse::<f64>() {
        return from_fractional_timestamp(timezone, seconds);
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(text) {
        return Ok(date.with_timezone(&timezone));
    }
    for format in [format, "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(date) = DateTime::parse_from_str(text, format) {
            return Ok(date.with_timezone(&timezone));
        }
        if let Ok(date) = NaiveDateTime::parse_from_str(text, format) {
            return localize(timezone, date);
        }
    }
    for format in [format, "%Y-%m-%d"] {
        if let Ok(date) = NaiveDate::parse_from_str(text, format) {
            return localize(timezone, date.and_hms_opt(0, 0, 0).ok_or_else(invalid)?);
        }
    }
    Err(invalid())
}

fn from_fractional_timestamp(timezone: Tz, seconds: f64) -> DatabaseResult<DateTime<Tz>> {
    let duration = std::time::Duration::try_from_secs_f64(seconds.abs()).map_err(|_| invalid())?;
    let duration = chrono::TimeDelta::from_std(duration).map_err(|_| invalid())?;
    let duration = if seconds < 0.0 { -duration } else { duration };
    DateTime::<Utc>::UNIX_EPOCH
        .checked_add_signed(duration)
        .map(|date| date.with_timezone(&timezone))
        .ok_or_else(invalid)
}

fn from_timestamp(timezone: Tz, seconds: i64, nanos: u32) -> DatabaseResult<DateTime<Tz>> {
    timezone
        .timestamp_opt(seconds, nanos)
        .single()
        .ok_or_else(invalid)
}

fn localize(timezone: Tz, date: NaiveDateTime) -> DatabaseResult<DateTime<Tz>> {
    timezone
        .from_local_datetime(&date)
        .earliest()
        .ok_or_else(invalid)
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("date attribute")
}
