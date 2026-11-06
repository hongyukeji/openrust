//! timelib 相对 production 扫描；全部贡献写入同一聚合状态。
//! 来源与完整版权许可见 SOURCES.md / LICENSE.timelib / LICENSE.carbon。

use super::format::{Cursor, weekday_number};
use super::state::{FirstLast, ParseState, RelativeUnit};
use super::{calendar_error, range_error};
use crate::Result;

pub(super) fn scan(cursor: &mut Cursor<'_>, state: &mut ParseState) -> Result<bool> {
    let start = cursor.position;
    if keyword(cursor, "first day of") {
        state.first_last(FirstLast::First)?;
        return Ok(true);
    }
    if keyword(cursor, "last day of") {
        state.first_last(FirstLast::Last)?;
        return Ok(true);
    }
    let first = cursor.source_word();
    match first {
        "now" => {
            state.noop()?;
            return Ok(true);
        }
        "today" | "midnight" => {
            state.reset_time(None)?;
            return Ok(true);
        }
        "tomorrow" => {
            state.reset_time(Some(1))?;
            return Ok(true);
        }
        "yesterday" => {
            state.reset_time(Some(-1))?;
            return Ok(true);
        }
        "noon" => {
            state.noon()?;
            return Ok(true);
        }
        "ago" => {
            state.ago()?;
            return Ok(true);
        }
        "front" | "back" => {
            if !cursor.rest().starts_with(" of ") {
                return Err(invalid());
            }
            cursor.position += 4;
            let mut hour = cursor.digits(1, 2)?;
            if hour > 24 {
                return Err(invalid());
            }
            if first == "front" {
                hour -= 1;
            }
            let before_meridian = cursor.position;
            cursor.whitespace();
            let meridian = cursor.word();
            if matches!(meridian, "pm" | "p.m.") && hour != 12 {
                hour += 12;
            } else if matches!(meridian, "am" | "a.m.") && hour == 12 {
                hour = 0;
            } else if !meridian.is_empty() && !matches!(meridian, "am" | "a.m." | "pm" | "p.m.") {
                cursor.position = before_meridian;
            }
            state.replace_time(hour, if first == "front" { 45 } else { 15 })?;
            return Ok(true);
        }
        _ => {}
    }
    if matches!(first, "weekday" | "weekdays") {
        state.weekday(1, None, 1, false)?;
        return Ok(true);
    }
    if let Some(day) = weekday_number(first) {
        state.weekday(day, None, 1, false)?;
        return Ok(true);
    }
    cursor.position = start;
    scan_amount(cursor, state)
}

fn scan_amount(cursor: &mut Cursor<'_>, state: &mut ParseState) -> Result<bool> {
    let start = cursor.position;
    let numeric = cursor
        .peek()
        .is_some_and(|c| c.is_ascii_digit() || matches!(c, '+' | '-'));
    let mut behavior = 0;
    let amount = if numeric {
        if let Ok(number) = relative_number(cursor) {
            number
        } else {
            cursor.position = start;
            return Ok(false);
        }
    } else {
        match cursor.source_word() {
            "next" => 1,
            "last" | "previous" => -1,
            "this" => {
                behavior = 1;
                0
            }
            word => {
                if let Some(value) = ordinal(word) {
                    value
                } else {
                    cursor.position = start;
                    return Ok(false);
                }
            }
        }
    };
    cursor.whitespace();
    let word = cursor.source_word();
    let unit = word.trim_end_matches('s');
    if let Some(day) = weekday_number(word) {
        cursor.whitespace();
        if keyword(cursor, "of") {
            state.ordinal_weekday(day, amount, behavior)?;
        } else {
            state.weekday(day, Some(amount), behavior, numeric)?;
        }
    } else if unit == "weekday" {
        state.weekdays(amount, numeric)?;
    } else if unit == "week" && !numeric {
        state.relative_week(amount)?;
    } else if let Some(unit) = relative_unit(word) {
        state.add(unit, amount)?;
    } else {
        cursor.position = start;
        return Ok(false);
    }
    Ok(true)
}
fn keyword(cursor: &mut Cursor<'_>, word: &str) -> bool {
    if cursor
        .rest()
        .strip_prefix(word)
        .is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphabetic()))
    {
        cursor.position += word.len();
        true
    } else {
        false
    }
}
fn relative_unit(word: &str) -> Option<RelativeUnit> {
    let unit = if matches!(word, "ms" | "µs") {
        word
    } else {
        word.trim_end_matches('s')
    };
    Some(match unit {
        "year" => RelativeUnit::Year,
        "month" => RelativeUnit::Month,
        "week" => RelativeUnit::Week,
        "fortnight" | "forthnight" => RelativeUnit::Fortnight,
        "day" => RelativeUnit::Day,
        "hour" => RelativeUnit::Hour,
        "min" | "minute" => RelativeUnit::Minute,
        "sec" | "second" => RelativeUnit::Second,
        "ms" | "msec" | "millisecond" => RelativeUnit::Millisecond,
        "µs" | "µsec" | "microsecond" | "usec" => RelativeUnit::Microsecond,
        _ => return None,
    })
}
fn ordinal(word: &str) -> Option<i64> {
    match word {
        "first" => Some(1),
        "second" => Some(2),
        "third" => Some(3),
        "fourth" => Some(4),
        "fifth" => Some(5),
        "sixth" => Some(6),
        "seventh" => Some(7),
        "eight" | "eighth" => Some(8),
        "ninth" => Some(9),
        "tenth" => Some(10),
        "eleventh" => Some(11),
        "twelfth" => Some(12),
        _ => None,
    }
}
fn relative_number(cursor: &mut Cursor<'_>) -> Result<i64> {
    let mut negative = false;
    while matches!(cursor.peek(), Some('+' | '-')) {
        if cursor.pop() == Some('-') {
            negative = !negative;
        }
    }
    cursor.whitespace();
    let value = cursor.digits(1, 13)?;
    if negative {
        value.checked_neg().ok_or_else(range_error)
    } else {
        Ok(value)
    }
}
fn invalid() -> crate::Error {
    calendar_error("input_date_invalid")
}
