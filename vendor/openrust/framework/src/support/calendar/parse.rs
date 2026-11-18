//! Carbon 构造前置转换与 timelib 绝对/相对日期解析。
//! 固定上游与版权许可见同目录 SOURCES.md / LICENSE.timelib / LICENSE.carbon。

use super::format::Cursor;
use super::lexer;
use super::state::ParseState;
use super::{CalendarInstant, InputDate, ResolvedTimezone, Result, calendar_error};
pub(super) fn parse_text(
    text: &str,
    now: CalendarInstant,
    zone: &ResolvedTimezone,
) -> Result<CalendarInstant> {
    let state = scan(text, now, zone, false)?;
    state.finish()
}

pub(super) fn parse_date(
    text: &str,
    now: CalendarInstant,
    zone: &ResolvedTimezone,
    frozen: bool,
) -> Result<InputDate> {
    let state = scan(text, now, zone, frozen)?;
    let zone = state.timezone().clone();
    Ok(InputDate::new(state.finish()?, zone))
}

pub(super) fn add_interval(
    date: &str,
    interval: &str,
    now: CalendarInstant,
    zone: &ResolvedTimezone,
) -> Result<(CalendarInstant, CalendarInstant)> {
    let parsed = scan(date, now, zone, false)?;
    let zone = parsed.timezone().clone();
    let start = parsed.finish()?;
    let end = scan(interval, start, &zone, false)?.finish_interval(start)?;
    Ok((start, end))
}

fn scan(
    text: &str,
    now: CalendarInstant,
    zone: &ResolvedTimezone,
    frozen: bool,
) -> Result<ParseState> {
    // 只规范化一次；后续生产匹配均使用游标切片，避免深前缀的平方次复制。
    let source = source_trim(text);
    let clean = source.to_ascii_lowercase();
    let mut cursor = Cursor::new(&clean);
    let current = zone.civil(now)?;
    let mut state = ParseState::new(current, zone.clone());
    state.reference(now, frozen);
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
        let timezone = lexer::timezone(cursor.rest(), &source[cursor.position..]);
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
    Ok(state)
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

fn invalid() -> super::CalendarError {
    calendar_error("input_date_invalid")
}
