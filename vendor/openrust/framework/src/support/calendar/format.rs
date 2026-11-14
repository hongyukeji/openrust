//! PHP createFromFormat 的顺序扫描与日期输出格式。
//! 固定上游与版权许可见同目录 SOURCES.md / LICENSE.timelib / LICENSE.carbon。

use super::civil::{days_from_civil, days_in_month, iso_week};
use super::zone::{ResolvedTimezone, display_offset};
use super::{CalendarInstant, Result, calendar_error, limit_error, range_error};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;

pub(super) const YEAR: u16 = 1;
pub(super) const MONTH: u16 = 2;
pub(super) const DAY: u16 = 4;
pub(super) const HOUR: u16 = 8;
pub(super) const MINUTE: u16 = 16;
pub(super) const SECOND: u16 = 32;
pub(super) const MICRO: u16 = 64;

pub(super) struct Cursor<'a> {
    pub text: &'a str,
    pub position: usize,
}
impl<'a> Cursor<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, position: 0 }
    }
    pub fn rest(&self) -> &'a str {
        self.text.get(self.position..).unwrap_or("")
    }
    pub fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    pub fn pop(&mut self) -> Option<char> {
        let value = self.peek()?;
        self.position += value.len_utf8();
        Some(value)
    }
    pub fn whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\u{a0}' | '\u{202f}')) {
            self.pop();
        }
    }
    pub fn digits(&mut self, minimum: usize, maximum: usize) -> Result<i64> {
        let start = self.position;
        while self.position - start < maximum
            && self
                .text
                .as_bytes()
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        if self.position - start < minimum {
            return Err(invalid());
        }
        self.text
            .get(start..self.position)
            .and_then(|value| value.parse().ok())
            .ok_or_else(invalid)
    }
    pub fn source_word(&mut self) -> &'a str {
        let start = self.position;
        while self.peek().is_some_and(char::is_alphabetic) {
            self.pop();
        }
        self.text.get(start..self.position).unwrap_or("")
    }
    pub fn word(&mut self) -> &'a str {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|value| value.is_alphabetic() || value == '.')
        {
            self.pop();
        }
        self.text.get(start..self.position).unwrap_or("")
    }
}

fn invalid() -> super::CalendarError {
    calendar_error("input_date_format_invalid")
}

pub(super) fn month_number(text: &str) -> Option<i64> {
    match text.trim_end_matches('.').to_ascii_lowercase().as_str() {
        "january" | "jan" | "i" => Some(1),
        "february" | "feb" | "ii" => Some(2),
        "march" | "mar" | "iii" => Some(3),
        "april" | "apr" | "iv" => Some(4),
        "may" | "v" => Some(5),
        "june" | "jun" | "vi" => Some(6),
        "july" | "jul" | "vii" => Some(7),
        "august" | "aug" | "viii" => Some(8),
        "september" | "sep" | "sept" | "ix" => Some(9),
        "october" | "oct" | "x" => Some(10),
        "november" | "nov" | "xi" => Some(11),
        "december" | "dec" | "xii" => Some(12),
        _ => None,
    }
}
pub(super) fn weekday_number(text: &str) -> Option<i64> {
    match text.trim_end_matches('.').to_ascii_lowercase().as_str() {
        "sunday" | "sun" | "sundays" => Some(0),
        "monday" | "mon" | "mondays" => Some(1),
        "tuesday" | "tue" | "tuesdays" => Some(2),
        "wednesday" | "wed" | "wednesdays" => Some(3),
        "thursday" | "thu" | "thursdays" => Some(4),
        "friday" | "fri" | "fridays" => Some(5),
        "saturday" | "sat" | "saturdays" => Some(6),
        _ => None,
    }
}

/// Renders the PHP date format tokens against the pinned time-zone rules.
#[allow(
    clippy::too_many_lines,
    reason = "Keeps the PHP date tokens in a single table"
)]
pub(crate) fn display(datetime: DateTime<Utc>, timezone: Tz, pattern: &str) -> Result<String> {
    if pattern.len() > super::MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    let instant = CalendarInstant::new(datetime.timestamp(), datetime.timestamp_subsec_micros())?;
    let zone = ResolvedTimezone::parse(timezone.name())?;
    let civil = zone.civil(instant)?;
    let weekday = usize::try_from(civil.weekday()?).map_err(|_| range_error())?;
    let offset = zone.offset_seconds(instant)?;
    let days = days_from_civil(civil.year, civil.month, civil.day);
    let day_of_year = days - days_from_civil(civil.year, 1, 1);
    let day_names = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let month_names = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month = usize::try_from(civil.month - 1).map_err(|_| range_error())?;
    let mut out = String::with_capacity(pattern.len());
    let mut tokens = pattern.chars();
    while let Some(token) = tokens.next() {
        let rendered = match token {
            '\\' => tokens.next().unwrap_or('\0').to_string(),
            'Y' => {
                if civil.year < 0 {
                    format!("-{:04}", i128::from(civil.year).abs())
                } else {
                    format!("{:04}", civil.year)
                }
            }
            'y' => format!("{:02}", civil.year % 100),
            'X' => format!(
                "{}{year:04}",
                if civil.year >= 0 { '+' } else { '-' },
                year = i128::from(civil.year).abs()
            ),
            'x' => {
                if civil.year < 0 {
                    format!("-{:04}", i128::from(civil.year).abs())
                } else if civil.year > 9999 {
                    format!("{:+}", civil.year)
                } else {
                    format!("{:04}", civil.year)
                }
            }
            'm' => format!("{:02}", civil.month),
            'n' => civil.month.to_string(),
            'd' => format!("{:02}", civil.day),
            'j' => civil.day.to_string(),
            'H' => format!("{:02}", civil.hour),
            'G' => civil.hour.to_string(),
            'h' => format!(
                "{:02}",
                if civil.hour % 12 == 0 {
                    12
                } else {
                    civil.hour % 12
                }
            ),
            'g' => {
                if civil.hour % 12 == 0 {
                    "12".into()
                } else {
                    (civil.hour % 12).to_string()
                }
            }
            'i' => format!("{:02}", civil.minute),
            's' => format!("{:02}", civil.second),
            'u' => format!("{:06}", civil.microsecond),
            'v' => format!("{:03}", civil.microsecond / 1000),
            'a' => {
                if civil.hour < 12 {
                    "am".into()
                } else {
                    "pm".into()
                }
            }
            'A' => {
                if civil.hour < 12 {
                    "AM".into()
                } else {
                    "PM".into()
                }
            }
            'w' => weekday.to_string(),
            'N' => {
                if weekday == 0 {
                    "7".into()
                } else {
                    weekday.to_string()
                }
            }
            'D' => day_names[weekday].get(..3).ok_or_else(range_error)?.into(),
            'l' => day_names[weekday].into(),
            'M' => month_names
                .get(month)
                .and_then(|name| name.get(..3))
                .ok_or_else(range_error)?
                .into(),
            'F' => month_names.get(month).ok_or_else(range_error)?.to_string(),
            'z' => day_of_year.to_string(),
            't' => days_in_month(civil.year, civil.month)?.to_string(),
            'L' => {
                if civil.year % 4 == 0 && (civil.year % 100 != 0 || civil.year % 400 == 0) {
                    "1".into()
                } else {
                    "0".into()
                }
            }
            'S' => {
                if (11..=13).contains(&civil.day) {
                    "th".into()
                } else {
                    match civil.day % 10 {
                        1 => "st",
                        2 => "nd",
                        3 => "rd",
                        _ => "th",
                    }
                    .into()
                }
            }
            'e' => timezone.name().into(),
            'P' => display_offset(offset),
            'O' => display_offset(offset).replace(':', ""),
            'p' => {
                if matches!(
                    zone.abbreviation(instant)?.as_str(),
                    "UTC" | "Z" | "GMT+0000"
                ) {
                    "Z".into()
                } else {
                    display_offset(offset)
                }
            }
            'Z' => offset.to_string(),
            'U' => datetime.timestamp().to_string(),
            'T' => zone.abbreviation(instant)?,
            'I' => u8::from(zone.is_dst(instant)?).to_string(),
            'B' => format!(
                "{:03}",
                (i128::from(datetime.timestamp()) + 3600).rem_euclid(86400) * 1000 / 86400
            ),
            'c' => format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}",
                civil.year,
                civil.month,
                civil.day,
                civil.hour,
                civil.minute,
                civil.second,
                display_offset(offset)
            ),
            'r' => format!(
                "{}, {:02} {} {:04} {:02}:{:02}:{:02} {}",
                &day_names[weekday][..3],
                civil.day,
                &month_names[month][..3],
                civil.year,
                civil.hour,
                civil.minute,
                civil.second,
                display_offset(offset).replace(':', "")
            ),
            'o' | 'W' => {
                let (year, week) = iso_week(civil.year, civil.month, civil.day)?;
                if token == 'o' {
                    year.to_string()
                } else {
                    format!("{week:02}")
                }
            }
            other => other.to_string(),
        };
        if out
            .len()
            .checked_add(rendered.len())
            .is_none_or(|len| len > super::MAX_CALENDAR_BYTES)
        {
            return Err(limit_error());
        }
        out.push_str(&rendered);
    }
    Ok(out)
}
