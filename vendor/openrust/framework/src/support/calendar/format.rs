//! PHP createFromFormat 的顺序扫描与日期输出格式。
//! 固定上游与版权许可见同目录 SOURCES.md / LICENSE.timelib / LICENSE.carbon。

use super::{Result, calendar_error};

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
