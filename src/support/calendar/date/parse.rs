//! Carbon 构造前置转换与 timelib 绝对/相对日期解析。
//! 固定上游与版权许可见同目录 SOURCES.md / LICENSE.timelib / LICENSE.carbon。

use super::format::Cursor;
use super::lexer;
use super::state::ParseState;
use super::{CalendarInstant, InputDate, ResolvedTimezone, calendar_error};
use crate::Result;
use serde_json::Value;

pub(super) fn parse(
    value: &Value,
    now: CalendarInstant,
    zone: &ResolvedTimezone,
    is_test: bool,
) -> Result<InputDate> {
    match value {
        Value::Bool(value) => Ok(InputDate::new(
            CalendarInstant::new(i64::from(*value), 0)?,
            ResolvedTimezone::parse("+00:00")?,
        )),
        Value::Number(number) => {
            let source = if number.is_f64() {
                lexer::number_format_timestamp(number.as_f64().ok_or_else(super::range_error)?)?
            } else {
                number.to_string()
            };
            timestamp(&source)
        }
        Value::String(text) => {
            if let Some(epoch) = text.strip_prefix('@') {
                return timestamp(epoch).or_else(|_| parse_text(text, now, zone, is_test));
            }
            let numeric = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
            if lexer::is_numeric(numeric)
                && !(text.len() <= 14
                    && !text.is_empty()
                    && text.bytes().all(|byte| byte.is_ascii_digit()))
            {
                return timestamp(text).or_else(|_| parse_text(text, now, zone, is_test));
            }
            parse_text(text, now, zone, is_test)
        }
        _ => Err(calendar_error("input_date_type_invalid")),
    }
}

fn timestamp(text: &str) -> Result<InputDate> {
    Ok(InputDate::new(
        lexer::carbon_timestamp(text)?,
        ResolvedTimezone::parse("+00:00")?,
    ))
}

fn parse_text(
    text: &str,
    now: CalendarInstant,
    zone: &ResolvedTimezone,
    is_test: bool,
) -> Result<InputDate> {
    // 只规范化一次；后续生产匹配均使用游标切片，避免深前缀的平方次复制。
    let clean = text.to_ascii_lowercase();
    let mut cursor = Cursor::new(source_trim(&clean));
    let current = zone.civil(now)?;
    let mut state = ParseState::new(current, zone.clone());
    state.reference(now, is_test);
    while !cursor.rest().is_empty() {
        while matches!(
            cursor.peek(),
            Some(' ' | '\t' | '\u{a0}' | '\u{202f}' | '\0' | '\n' | '.' | ',')
        ) {
            cursor.pop();
        }
        if cursor.rest().is_empty() {
            break;
        }
        if super::relative::scan(&mut cursor, &mut state)? {
            continue;
        }
        let absolute = lexer::scan(cursor.rest(), current);
        let timezone = lexer::timezone(cursor.rest());
        if let Some((length, timezone)) = timezone
            && absolute
                .as_ref()
                .is_none_or(|token| length > token.consumed)
        {
            state.zone(timezone)?;
            cursor.position += length;
        } else if let Some(token) = absolute {
            cursor.position += token.consumed;
            state.absolute(&token)?;
        } else {
            return Err(invalid());
        }
    }
    state.finish()
}

fn source_trim(text: &str) -> &str {
    // timelib 外缘按 C isspace 去除；全空白时保留最后一byte，交源scanner判定。
    let mut start = 0;
    let mut end = text.len();
    while start + 1 < end && text.as_bytes()[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start + 1 && text.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    text.get(start..end).unwrap_or("")
}

fn invalid() -> crate::Error {
    calendar_error("input_date_invalid")
}
