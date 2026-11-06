//! PHP createFromFormat 的顺序扫描与日期输出格式。
//! 固定上游与版权许可见同目录 SOURCES.md / LICENSE.timelib / LICENSE.carbon。

use super::civil::{Civil, days_from_civil, days_in_month};
use super::{
    CalendarInstant, InputDate, ResolvedTimezone, calendar_error, limit_error, range_error,
};
use crate::Result;

pub(super) const YEAR: u16 = 1;
pub(super) const MONTH: u16 = 2;
pub(super) const DAY: u16 = 4;
pub(super) const HOUR: u16 = 8;
pub(super) const MINUTE: u16 = 16;
pub(super) const SECOND: u16 = 32;
pub(super) const MICRO: u16 = 64;
const ALL: u16 = 127;

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
    pub fn signed(&mut self, maximum: usize) -> Result<i64> {
        let start = self.position;
        if matches!(self.peek(), Some('-' | '+')) {
            self.pop();
        }
        let digits = self.position;
        while self.position - digits < maximum
            && self
                .text
                .as_bytes()
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        if self.position == digits {
            return Err(invalid());
        }
        self.text
            .get(start..self.position)
            .and_then(|text| text.parse::<i64>().ok())
            .ok_or_else(range_error)
    }
    pub fn source_word(&mut self) -> &'a str {
        let start = self.position;
        while self.peek().is_some_and(char::is_alphabetic) {
            self.pop();
        }
        self.text.get(start..self.position).unwrap_or("")
    }
    fn skip_byte(&mut self) -> Result<()> {
        if self.position >= self.text.len() {
            return Err(invalid());
        }
        self.position += 1;
        Ok(())
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

// PHP 格式token顺序保持在同一match，避免格式状态在多个解析器间漂移。
#[allow(clippy::too_many_lines)]
pub(super) fn parse(
    text: &str,
    pattern: &str,
    now: Civil,
    default_zone: &ResolvedTimezone,
) -> Result<InputDate> {
    if text.contains('\0') {
        return Err(invalid());
    }
    let expanded = expand_c(pattern);
    let mut cursor = Cursor::new(text);
    let mut civil = now;
    let mut zone = default_zone.clone();
    let mut mask = 0u16;
    let mut seen_time = false;
    let mut weekday = None;
    let mut meridian = None;
    let mut allow_extra = false;
    let mut patterns = expanded.chars();
    while let Some(token) = patterns.next() {
        match token {
            '\\' => {
                let expected = patterns.next().ok_or_else(invalid)?;
                let actual = cursor.pop().ok_or_else(invalid)?;
                if actual != expected {
                    return Err(invalid());
                }
            }
            '!' => {
                civil = Civil::epoch();
                mask = ALL;
            }
            '|' => reset_unset(&mut civil, mask),
            '+' => allow_extra = true,
            ' ' => cursor.whitespace(),
            '?' => cursor.skip_byte()?,
            '*' => {
                cursor.skip_byte()?;
                while cursor
                    .text
                    .as_bytes()
                    .get(cursor.position)
                    .is_some_and(|byte| {
                        !byte.is_ascii_digit()
                            && !matches!(
                                *byte,
                                b' ' | b'\t' | b'.' | b',' | b':' | b';' | b'/' | b'-'
                            )
                    })
                {
                    cursor.position += 1;
                }
            }
            '#' => {
                if !cursor.pop().is_some_and(is_separator) {
                    return Err(invalid());
                }
            }
            'Y' => {
                civil.year = cursor.digits(1, 4)?;
                mask |= YEAR;
            }
            'x' | 'X' => {
                civil.year = cursor.signed(19)?;
                mask |= YEAR;
            }
            'y' => {
                let year = cursor.digits(1, 2)?;
                civil.year = year + if year < 70 { 2000 } else { 1900 };
                mask |= YEAR;
            }
            'm' | 'n' => {
                civil.month = cursor.digits(1, 2)?;
                mask |= MONTH;
            }
            'd' | 'j' => {
                civil.day = cursor.digits(1, 2)?;
                mask |= DAY;
            }
            'M' | 'F' => {
                civil.month = month_number(cursor.word()).ok_or_else(invalid)?;
                mask |= MONTH;
            }
            'D' | 'l' => {
                weekday = Some(weekday_number(cursor.word()).ok_or_else(invalid)?);
            }
            'S' => {
                if cursor.position == cursor.text.len() {
                    return Err(invalid());
                }
                if cursor.rest().get(..2).is_some_and(|suffix| {
                    ["nd", "rd", "st", "th"]
                        .iter()
                        .any(|known| suffix.eq_ignore_ascii_case(known))
                }) {
                    cursor.position += 2;
                }
            }
            'z' => {
                if mask & YEAR == 0 {
                    return Err(invalid());
                }
                civil.month = 1;
                civil.day = cursor
                    .digits(1, 3)?
                    .checked_add(1)
                    .ok_or_else(range_error)?;
                mask |= MONTH | DAY;
            }
            'H' | 'G' | 'h' | 'g' => {
                start_time(&mut civil, &mut seen_time, mask);
                civil.hour = cursor.digits(1, 2)?;
                if matches!(token, 'h' | 'g') && (civil.hour == 0 || civil.hour > 12) {
                    return Err(invalid());
                }
                mask |= HOUR;
            }
            'i' => {
                start_time(&mut civil, &mut seen_time, mask);
                civil.minute = cursor.digits(1, 2)?;
                mask |= MINUTE;
            }
            's' => {
                start_time(&mut civil, &mut seen_time, mask);
                civil.second = cursor.digits(1, 2)?;
                mask |= SECOND;
            }
            'u' | 'v' => {
                start_time(&mut civil, &mut seen_time, mask);
                let start = cursor.position;
                let digits = cursor.digits(1, if token == 'u' { 6 } else { 3 })?;
                let count = cursor.position - start;
                civil.microsecond =
                    digits * 10i64.pow(u32::try_from(6 - count).map_err(|_| invalid())?);
                mask |= MICRO;
            }
            'a' | 'A' => {
                let word = cursor.word().replace('.', "").to_ascii_lowercase();
                meridian = Some(match word.as_str() {
                    "am" => false,
                    "pm" => true,
                    _ => return Err(invalid()),
                });
            }
            'U' => {
                let seconds = cursor.signed(24)?;
                let micros = u32::try_from(civil.microsecond).map_err(|_| range_error())?;
                civil = Civil::from_instant(CalendarInstant::new(seconds, micros)?, 0)?;
                zone = ResolvedTimezone::parse("+00:00")?;
                mask = ALL;
            }
            'e' | 'T' | 'P' | 'p' | 'O' => {
                let start = cursor.position;
                while cursor.peek().is_some_and(|value| {
                    value.is_ascii_alphanumeric() || matches!(value, '_' | '/' | '+' | '-' | ':')
                }) {
                    cursor.pop();
                }
                let name = cursor
                    .text
                    .get(start..cursor.position)
                    .ok_or_else(invalid)?;
                zone = ResolvedTimezone::parse(name)?;
            }
            other => {
                let actual = cursor.pop().ok_or_else(invalid)?;
                if actual != other {
                    return Err(invalid());
                }
            }
        }
    }
    if cursor.position != cursor.text.len() && !allow_extra {
        return Err(invalid());
    }
    if let Some(pm) = meridian {
        if mask & HOUR == 0 {
            return Err(invalid());
        }
        civil.hour = civil.hour % 12 + if pm { 12 } else { 0 };
    }
    if let Some(target) = weekday {
        civil = civil.normalize()?;
        civil.add_days((target - civil.weekday()?).rem_euclid(7))?;
    }
    Ok(InputDate::new(zone.resolve(civil)?, zone))
}

fn start_time(civil: &mut Civil, seen: &mut bool, mask: u16) {
    if !*seen {
        if mask & HOUR == 0 {
            civil.hour = 0;
        }
        if mask & MINUTE == 0 {
            civil.minute = 0;
        }
        if mask & SECOND == 0 {
            civil.second = 0;
        }
        if mask & MICRO == 0 {
            civil.microsecond = 0;
        }
        *seen = true;
    }
}
fn reset_unset(civil: &mut Civil, mask: u16) {
    let epoch = Civil::epoch();
    if mask & YEAR == 0 {
        civil.year = epoch.year;
    }
    if mask & MONTH == 0 {
        civil.month = epoch.month;
    }
    if mask & DAY == 0 {
        civil.day = epoch.day;
    }
    if mask & HOUR == 0 {
        civil.hour = 0;
    }
    if mask & MINUTE == 0 {
        civil.minute = 0;
    }
    if mask & SECOND == 0 {
        civil.second = 0;
    }
    if mask & MICRO == 0 {
        civil.microsecond = 0;
    }
}
fn expand_c(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut escaped = false;
    for value in pattern.chars() {
        if escaped {
            out.push(value);
            escaped = false;
        } else if value == '\\' {
            out.push(value);
            escaped = true;
        } else if value == 'c' {
            out.push_str("Y-m-d\\TH:i:sP");
        } else {
            out.push(value);
        }
    }
    out
}
fn is_separator(value: char) -> bool {
    matches!(value, ';' | ':' | '/' | '.' | ',' | '-' | '(' | ')')
}
fn invalid() -> crate::Error {
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

// PHP输出格式在同一token表中维护，防止转换函数形成互相矛盾的格式解释。
#[allow(clippy::too_many_lines)]
pub(super) fn display(date: &InputDate, pattern: &str) -> Result<String> {
    if pattern.len() > super::MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    let civil = date.timezone.civil(date.instant)?;
    let weekday = usize::try_from(civil.weekday()?).map_err(|_| range_error())?;
    let offset = date.timezone.offset_seconds(date.instant)?;
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
            '\\' => tokens
                .next()
                .map_or_else(String::new, |value| value.to_string()),
            'Y' => {
                if civil.year < 0 {
                    format!("-{:04}", i128::from(civil.year).abs())
                } else {
                    format!("{:04}", civil.year)
                }
            }
            'y' => format!("{:02}", civil.year.rem_euclid(100)),
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
            'e' => date.timezone.display_name(),
            'P' => super::zone::display_offset(offset),
            'O' => super::zone::display_offset(offset).replace(':', ""),
            'p' => {
                if (0..60).contains(&offset) {
                    "Z".into()
                } else {
                    super::zone::display_offset(offset)
                }
            }
            'Z' => offset.to_string(),
            'U' => date.timestamp().to_string(),
            'T' => date.timezone.abbreviation(date.instant)?,
            'I' => u8::from(date.timezone.is_dst(date.instant)?).to_string(),
            'B' => format!(
                "{:03}",
                (i128::from(date.timestamp()) + 3600).rem_euclid(86400) * 1000 / 86400
            ),
            'c' => display(date, "Y-m-d\\TH:i:sP")?,
            'r' => display(date, "D, d M Y H:i:s O")?,
            'o' | 'W' => {
                let (year, week) = super::civil::iso_week(civil.year, civil.month, civil.day)?;
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
