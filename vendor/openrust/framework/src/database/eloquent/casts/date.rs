use std::fmt::Write as _;

use jiff::{
    SignedDuration, Timestamp, Zoned,
    civil::{Date as CivilDate, DateTime},
    tz::TimeZone,
};

use crate::database::{DatabaseError, DatabaseResult, Value};

use super::AttributeCast;

pub(super) fn read(
    cast: AttributeCast,
    raw: &Value,
    timezone: TimeZone,
    format: &str,
) -> DatabaseResult<Value> {
    if matches!(raw, Value::Null) {
        return Ok(Value::Null);
    }
    let mut date = parse(raw, timezone.clone(), format)?;
    if matches!(cast, AttributeCast::Timestamp) {
        return Ok(Value::Integer(crate::support::timestamp_seconds(
            date.timestamp(),
        )));
    }
    if matches!(cast, AttributeCast::Date | AttributeCast::DateWithFormat(_)) {
        let midnight = date.datetime().start_of_day();
        date = localize(timezone, midnight)?;
    }
    Ok(date
        .timestamp()
        .strftime("%Y-%m-%dT%H:%M:%S%.6fZ")
        .to_string()
        .into())
}

pub(super) fn store(raw: Value, timezone: TimeZone, format: &str) -> DatabaseResult<Value> {
    if matches!(raw, Value::Null) {
        return Ok(raw);
    }
    format_for_storage(parse(&raw, timezone, format)?, format)
}

pub(super) fn equivalent(
    current: &Value,
    original: &Value,
    timezone: TimeZone,
    format: &str,
) -> DatabaseResult<bool> {
    Ok(
        format_for_storage(parse(current, timezone.clone(), format)?, format)?
            == format_for_storage(parse(original, timezone, format)?, format)?,
    )
}

pub(in crate::database::eloquent) fn format_for_storage(
    date: Zoned,
    format: &str,
) -> DatabaseResult<Value> {
    let mut output = String::new();
    write!(&mut output, "{}", date.strftime(format)).map_err(|_| invalid())?;
    Ok(output.into())
}

fn parse(raw: &Value, timezone: TimeZone, format: &str) -> DatabaseResult<Zoned> {
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

fn parse_text(text: &str, timezone: TimeZone, format: &str) -> DatabaseResult<Zoned> {
    if let Ok(seconds) = text.parse::<i64>() {
        return from_timestamp(timezone, seconds, 0);
    }
    if let Ok(seconds) = text.parse::<f64>() {
        return from_fractional_timestamp(timezone, seconds);
    }
    if let Ok(date) = text.parse::<Timestamp>() {
        return Ok(date.to_zoned(timezone));
    }
    for format in [format, "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(date) = Timestamp::strptime(format, text) {
            return Ok(date.to_zoned(timezone));
        }
        if let Ok(date) = DateTime::strptime(format, text) {
            return localize(timezone, date);
        }
    }
    for format in [format, "%Y-%m-%d"] {
        if let Ok(date) = CivilDate::strptime(format, text) {
            return localize(timezone, date.at(0, 0, 0, 0));
        }
    }
    Err(invalid())
}

fn from_fractional_timestamp(timezone: TimeZone, seconds: f64) -> DatabaseResult<Zoned> {
    let duration = SignedDuration::try_from_secs_f64(seconds).map_err(|_| invalid())?;
    Timestamp::UNIX_EPOCH
        .checked_add(duration)
        .map(|date| date.to_zoned(timezone))
        .map_err(|_| invalid())
}

fn from_timestamp(timezone: TimeZone, seconds: i64, nanos: u32) -> DatabaseResult<Zoned> {
    Timestamp::new(seconds, i32::try_from(nanos).map_err(|_| invalid())?)
        .map(|date| date.to_zoned(timezone))
        .map_err(|_| invalid())
}

fn localize(timezone: TimeZone, date: DateTime) -> DatabaseResult<Zoned> {
    let ambiguous = timezone.to_ambiguous_zoned(date);
    if matches!(ambiguous.offset(), jiff::tz::AmbiguousOffset::Gap { .. }) {
        return Err(invalid());
    }
    ambiguous.earlier().map_err(|_| invalid())
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("date attribute")
}
