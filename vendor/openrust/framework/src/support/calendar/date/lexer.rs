//! 固定 timelib production 的默认词法扫描，不经 `createFromFormat` 候选回溯。
//! 来源：PHP 8.5.0 `parse_date.re`、Carbon 3.8.4 `Timestamp.php`；许可见 SOURCES.md。

use super::civil::Civil;
use super::format::{self, Cursor};
use super::{CalendarInstant, ResolvedTimezone, range_error};
use crate::Result;

const DATE: u16 = format::YEAR | format::MONTH | format::DAY;
const TIME: u16 = format::HOUR | format::MINUTE | format::SECOND | format::MICRO;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProductionTag {
    Date,
    Month,
    Year,
    Time,
    GnuTime,
    Timestamp,
}

pub(super) struct Token {
    pub tag: ProductionTag,
    pub civil: Civil,
    pub mask: u16,
    pub consumed: usize,
    // isoweek 将 offset 写入 relative.d，而非提前变为另一个绝对日期。
    pub relative_day: Option<i64>,
}

pub(super) fn scan(text: &str, current: Civil) -> Option<Token> {
    let mut best = None;
    offer(&mut best, time(text, current));
    numeric_dates(text, current, &mut best);
    textual_dates(text, current, &mut best);
    offer(&mut best, week_or_ordinal(text, current));
    offer(&mut best, embedded_timestamp(text));
    offer(&mut best, clf(text, current));
    offer(&mut best, exif(text, current));
    offer(&mut best, short_date_time(text, current));
    if let Some(date) = &best
        && date.tag == ProductionTag::Date
        && date.mask & TIME == 0
        && text
            .get(date.consumed..)
            .is_some_and(|rest| rest.starts_with('t'))
        && let Some(clock) = time(text.get(date.consumed..)?, current)
    {
        let combined = combine(date, &clock);
        offer(&mut best, Some(combined));
    }
    best
}
fn offer(best: &mut Option<Token>, candidate: Option<Token>) {
    if let Some(candidate) = candidate
        && best
            .as_ref()
            .is_none_or(|old| candidate.consumed > old.consumed)
    {
        *best = Some(candidate);
    }
}
fn token(cursor: &Cursor<'_>, civil: Civil, mask: u16, tag: ProductionTag) -> Token {
    Token {
        tag,
        civil,
        mask,
        consumed: cursor.position,
        relative_day: None,
    }
}

// 共享 Cursor 的源语法原语：不分配剩余字符串，也不重新小写整个输入。
impl Cursor<'_> {
    fn source_literal(&mut self, expected: char) -> Option<()> {
        (self.pop()? == expected).then_some(())
    }
    fn source_number(&mut self, minimum: usize, maximum: usize, upper: i64) -> Option<i64> {
        let start = self.position;
        let mut length = 0;
        while length < maximum
            && self
                .text
                .as_bytes()
                .get(start + length)
                .is_some_and(u8::is_ascii_digit)
        {
            length += 1;
        }
        while length >= minimum {
            let value = self.text.get(start..start + length)?.parse::<i64>().ok()?;
            if value <= upper {
                self.position = start + length;
                return Some(value);
            }
            length -= 1;
        }
        None
    }
    fn source_separator(&mut self) {
        while self
            .peek()
            .is_some_and(|c| matches!(c, ' ' | '\t' | '.' | '-'))
        {
            self.pop();
        }
    }
    fn source_space(&mut self) -> bool {
        let start = self.position;
        while matches!(self.peek(), Some(' ' | '\t' | '\u{a0}' | '\u{202f}')) {
            self.pop();
        }
        self.position != start
    }
    fn source_year(&mut self, minimum: usize, maximum: usize, window: bool) -> Option<i64> {
        let start = self.position;
        let year = self.source_number(minimum, maximum, i64::MAX)?;
        Some(if window && self.position - start < 4 && year < 100 {
            year + if year < 70 { 2000 } else { 1900 }
        } else {
            year
        })
    }
    fn source_month(&mut self, roman: bool, abbreviation_only: bool) -> Option<i64> {
        let start = self.position;
        let word = self.source_word();
        if abbreviation_only && word.len() > 4 {
            return None;
        }
        if !roman && word.bytes().all(|byte| matches!(byte, b'i' | b'v' | b'x')) {
            return None;
        }
        let value = format::month_number(word)?;
        self.position = start + word.len();
        Some(value)
    }
    fn source_day(&mut self, padded: bool, suffix: bool) -> Option<i64> {
        let day = self.source_number(if padded { 2 } else { 1 }, 2, 31)?;
        if suffix
            && ["st", "nd", "rd", "th"]
                .iter()
                .any(|suffix| self.rest().starts_with(suffix))
        {
            self.position += 2;
        }
        Some(day)
    }
}

#[derive(Clone, Copy)]
struct Year {
    minimum: usize,
    maximum: usize,
    sign: bool,
    window: bool,
}
impl Year {
    fn take(self, cursor: &mut Cursor<'_>) -> Option<i64> {
        let start = cursor.position;
        if self.sign {
            match cursor.pop()? {
                '+' | '-' => {}
                _ => {
                    cursor.position = start;
                    return None;
                }
            }
            cursor.source_number(self.minimum, self.maximum, i64::MAX)?;
            cursor.text.get(start..cursor.position)?.parse::<i64>().ok()
        } else {
            cursor.source_year(self.minimum, self.maximum, self.window)
        }
    }
}
const YEAR4: Year = Year {
    minimum: 4,
    maximum: 4,
    sign: false,
    window: false,
};
const YEAR: Year = Year {
    minimum: 1,
    maximum: 4,
    sign: false,
    window: true,
};

fn numeric_dates(text: &str, current: Civil, best: &mut Option<Token>) {
    for year in [
        YEAR4,
        Year {
            sign: true,
            ..YEAR4
        },
        Year {
            minimum: 2,
            maximum: 2,
            window: true,
            ..YEAR4
        },
        Year {
            minimum: 5,
            maximum: 19,
            sign: true,
            ..YEAR4
        },
        YEAR,
    ] {
        offer(best, ymd(text, current, year, '-', year.minimum != 1));
    }
    offer(best, ymd(text, current, YEAR4, '/', false));
    offer(best, american(text, current));
    offer(best, pointed(text, current, 4));
    offer(best, pointed(text, current, 2));
    offer(best, compact_date(text, current));
    offer(best, year_month(text, current));
    let mut cursor = Cursor::new(text);
    if let Some(year) = cursor.source_year(4, 4, false) {
        offer(
            best,
            Some(token(
                &cursor,
                Civil { year, ..current },
                format::YEAR,
                ProductionTag::Year,
            )),
        );
    }
}
fn ymd(text: &str, current: Civil, year: Year, delimiter: char, padded: bool) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let year = year.take(&mut cursor)?;
    cursor.source_literal(delimiter)?;
    let month = cursor.source_number(if padded { 2 } else { 1 }, 2, 12)?;
    cursor.source_literal(delimiter)?;
    let day = cursor.source_day(padded, !padded)?;
    if delimiter == '/' && cursor.peek() == Some('/') {
        cursor.pop();
    }
    Some(token(
        &cursor,
        Civil {
            year,
            month,
            day,
            ..current
        },
        DATE,
        ProductionTag::Date,
    ))
}
fn american(text: &str, current: Civil) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let month = cursor.source_number(1, 2, 12)?;
    cursor.source_literal('/')?;
    let day = cursor.source_day(false, true)?;
    let mut civil = Civil {
        month,
        day,
        ..current
    };
    let mut mask = format::MONTH | format::DAY;
    if cursor.peek() == Some('/') {
        cursor.pop();
        civil.year = cursor.source_year(1, 4, true)?;
        mask |= format::YEAR;
    }
    Some(token(&cursor, civil, mask, ProductionTag::Date))
}
fn pointed(text: &str, current: Civil, width: usize) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let day = cursor.source_day(false, true)?;
    let first = cursor.pop()?;
    if !matches!(first, '.' | '\t' | '-') || (width == 2 && first == '-') {
        return None;
    }
    let month = cursor.source_number(1, 2, 12)?;
    let second = cursor.pop()?;
    if !matches!(second, '.' | '-') || (width == 2 && second != '.') {
        return None;
    }
    let year = cursor.source_year(width, width, width == 2)?;
    Some(token(
        &cursor,
        Civil {
            year,
            month,
            day,
            ..current
        },
        DATE,
        ProductionTag::Date,
    ))
}
fn compact_date(text: &str, current: Civil) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let year = cursor.source_year(4, 4, false)?;
    let month = cursor.source_number(2, 2, 12)?;
    let day = cursor.source_day(true, false)?;
    Some(token(
        &cursor,
        Civil {
            year,
            month,
            day,
            ..current
        },
        DATE,
        ProductionTag::Date,
    ))
}
fn year_month(text: &str, current: Civil) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let year = cursor.source_year(4, 4, false)?;
    cursor.source_literal('-')?;
    let month = cursor.source_number(1, 2, 12)?;
    Some(token(
        &cursor,
        Civil {
            year,
            month,
            day: 1,
            ..current
        },
        DATE,
        ProductionTag::Date,
    ))
}

fn textual_dates(text: &str, current: Civil, best: &mut Option<Token>) {
    offer(best, day_month(text, current, true));
    offer(best, day_month(text, current, false));
    offer(best, month_day(text, current, true));
    offer(best, month_day(text, current, false));
    offer(best, month_year(text, current, false));
    offer(best, month_year(text, current, true));
    offer(best, postgres(text, current, false));
    offer(best, postgres(text, current, true));
    let mut cursor = Cursor::new(text);
    if let Some(month) = cursor.source_month(false, false) {
        offer(
            best,
            Some(token(
                &cursor,
                Civil { month, ..current },
                format::MONTH,
                ProductionTag::Month,
            )),
        );
    }
}
fn day_month(text: &str, current: Civil, with_year: bool) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let day = cursor.source_day(false, true)?;
    cursor.source_separator();
    let month = cursor.source_month(true, false)?;
    let mut civil = Civil {
        month,
        day,
        ..current
    };
    let mut mask = format::MONTH | format::DAY;
    if with_year {
        cursor.source_separator();
        civil.year = cursor.source_year(1, 4, true)?;
        mask |= format::YEAR;
    }
    Some(token(&cursor, civil, mask, ProductionTag::Date))
}
fn month_day(text: &str, current: Civil, with_year: bool) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let month = cursor.source_month(true, false)?;
    cursor.source_separator();
    let day = cursor.source_day(false, true)?;
    let mut civil = Civil {
        month,
        day,
        ..current
    };
    let mut mask = format::MONTH | format::DAY;
    let separator = cursor.position;
    while cursor.peek().is_some_and(|c| {
        matches!(
            c,
            ',' | '.' | 's' | 't' | 'n' | 'd' | 'r' | 'h' | '\t' | ' '
        )
    }) {
        cursor.pop();
    }
    if with_year {
        if separator == cursor.position {
            return None;
        }
        civil.year = cursor.source_year(1, 4, true)?;
        mask |= format::YEAR;
    } else if separator == cursor.position && !cursor.rest().is_empty() {
        return None;
    }
    Some(token(&cursor, civil, mask, ProductionTag::Date))
}
fn month_year(text: &str, current: Civil, reverse: bool) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let (year, month) = if reverse {
        let year = cursor.source_year(4, 4, false)?;
        cursor.source_separator();
        (year, cursor.source_month(true, false)?)
    } else {
        let month = cursor.source_month(true, false)?;
        cursor.source_separator();
        (cursor.source_year(4, 4, false)?, month)
    };
    Some(token(
        &cursor,
        Civil {
            year,
            month,
            day: 1,
            ..current
        },
        DATE,
        ProductionTag::Date,
    ))
}
fn postgres(text: &str, current: Civil, reverse: bool) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let (year, month, day) = if reverse {
        let year = cursor.source_year(1, 4, true)?;
        cursor.source_literal('-')?;
        let month = cursor.source_month(false, true)?;
        cursor.source_literal('-')?;
        (year, month, cursor.source_day(true, false)?)
    } else {
        let month = cursor.source_month(false, true)?;
        cursor.source_literal('-')?;
        let day = cursor.source_day(true, false)?;
        cursor.source_literal('-')?;
        (cursor.source_year(1, 4, true)?, month, day)
    };
    Some(token(
        &cursor,
        Civil {
            year,
            month,
            day,
            ..current
        },
        DATE,
        ProductionTag::Date,
    ))
}

fn combine(date: &Token, clock: &Token) -> Token {
    Token {
        tag: ProductionTag::Date,
        civil: Civil {
            hour: clock.civil.hour,
            minute: clock.civil.minute,
            second: clock.civil.second,
            microsecond: clock.civil.microsecond,
            ..date.civil
        },
        mask: date.mask | clock.mask,
        consumed: date.consumed + clock.consumed,
        relative_day: date.relative_day,
    }
}
fn short_date_time(text: &str, current: Civil) -> Option<Token> {
    let date = month_day(text, current, false)?;
    let rest = text.get(date.consumed..)?;
    let mut look = Cursor::new(rest);
    if look.peek() == Some('t') {
        look.pop();
    }
    look.source_number(1, 2, 24)?;
    if !matches!(look.peek(), Some(':' | '.')) {
        return None;
    }
    Some(combine(&date, &time(rest, current)?))
}

fn full_time(cursor: &mut Cursor<'_>, civil: &mut Civil) -> Option<()> {
    civil.hour = cursor.source_number(2, 2, 24)?;
    cursor.source_literal(':')?;
    civil.minute = cursor.source_number(2, 2, 59)?;
    cursor.source_literal(':')?;
    civil.second = cursor.source_number(2, 2, 60)?;
    civil.microsecond = 0;
    Some(())
}
fn clf(text: &str, current: Civil) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let day = cursor.source_day(false, true)?;
    cursor.source_literal('/')?;
    let month = cursor.source_month(false, true)?;
    cursor.source_literal('/')?;
    let year = cursor.source_year(4, 4, false)?;
    cursor.source_literal(':')?;
    let mut civil = Civil {
        year,
        month,
        day,
        ..current
    };
    full_time(&mut cursor, &mut civil)?;
    // clf 的 zone correction 是必要部分；仍由外层共享 zone scanner 消费与计数。
    let mut look = Cursor::new(cursor.rest());
    if !look.source_space() || !matches!(look.peek(), Some('+' | '-')) {
        return None;
    }
    Some(token(&cursor, civil, DATE | TIME, ProductionTag::Date))
}
fn exif(text: &str, current: Civil) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let year = cursor.source_year(4, 4, false)?;
    cursor.source_literal(':')?;
    let month = cursor.source_number(2, 2, 12)?;
    cursor.source_literal(':')?;
    let day = cursor.source_day(true, false)?;
    cursor.source_literal(' ')?;
    let mut civil = Civil {
        year,
        month,
        day,
        ..current
    };
    full_time(&mut cursor, &mut civil)?;
    Some(token(&cursor, civil, DATE | TIME, ProductionTag::Date))
}

fn week_or_ordinal(text: &str, current: Civil) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let year = cursor.source_year(4, 4, false)?;
    if matches!(cursor.peek(), Some('-' | '.')) {
        cursor.pop();
    }
    if cursor.peek() == Some('w') {
        cursor.pop();
        let week = cursor.source_number(2, 2, 53)?;
        if week == 0 {
            return None;
        }
        let day = if cursor.peek() == Some('-') {
            cursor.pop();
            cursor.source_number(1, 1, 7)?
        } else {
            cursor.source_number(1, 1, 7).unwrap_or(1)
        };
        let january = Civil {
            year,
            month: 1,
            day: 4,
            ..current
        };
        let monday = (january.weekday().ok()? + 6).rem_euclid(7);
        let mut value = token(
            &cursor,
            Civil {
                year,
                month: 1,
                day: 1,
                ..current
            },
            DATE,
            ProductionTag::Date,
        );
        value.relative_day = Some(3 - monday + (week - 1) * 7 + day - 1);
        Some(value)
    } else {
        let day = cursor.source_number(3, 3, 366)?;
        if day == 0 {
            return None;
        }
        Some(token(
            &cursor,
            Civil {
                year,
                month: 1,
                day,
                ..current
            },
            DATE,
            ProductionTag::Date,
        ))
    }
}

fn time(text: &str, current: Civil) -> Option<Token> {
    let mut best = None;
    offer(&mut best, clock_time(text, current, true));
    offer(&mut best, clock_time(text, current, false));
    offer(&mut best, compact_time(text, current, false));
    offer(&mut best, compact_time(text, current, true));
    best
}
fn compact_time(text: &str, current: Civil, seconds: bool) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    if cursor.peek() == Some('t') {
        cursor.pop();
    }
    let hour = cursor.source_number(2, 2, 24)?;
    let minute = cursor.source_number(2, 2, 59)?;
    let second = if seconds {
        cursor.source_number(2, 2, 60)?
    } else {
        0
    };
    Some(token(
        &cursor,
        Civil {
            hour,
            minute,
            second,
            microsecond: 0,
            ..current
        },
        TIME,
        if seconds {
            ProductionTag::Time
        } else {
            ProductionTag::GnuTime
        },
    ))
}
fn clock_time(text: &str, current: Civil, meridian: bool) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    let has_t = cursor.peek() == Some('t');
    if has_t {
        cursor.pop();
    }
    if has_t && meridian {
        return None;
    }
    let hour = cursor.source_number(1, 2, if meridian { 12 } else { 24 })?;
    if meridian && hour == 0 {
        return None;
    }
    let mut civil = Civil {
        hour,
        minute: 0,
        second: 0,
        microsecond: 0,
        ..current
    };
    let separated = matches!(cursor.peek(), Some(':' | '.'));
    if separated {
        cursor.pop();
        civil.minute = cursor.source_number(if meridian { 2 } else { 1 }, 2, 59)?;
        if matches!(cursor.peek(), Some(':' | '.')) {
            cursor.pop();
            civil.second = cursor.source_number(if meridian { 2 } else { 1 }, 2, 60)?;
            if (cursor.peek() == Some('.') || (meridian && cursor.peek() == Some(':')))
                && cursor
                    .rest()
                    .as_bytes()
                    .get(1)
                    .is_some_and(u8::is_ascii_digit)
            {
                cursor.pop();
                civil.microsecond = fraction(&mut cursor)?;
            }
        }
    }
    if meridian {
        cursor.source_space();
        let pm = match cursor.pop()? {
            'a' => false,
            'p' => true,
            _ => return None,
        };
        if cursor.peek() == Some('.') {
            cursor.pop();
        }
        cursor.source_literal('m')?;
        if cursor.peek() == Some('.') {
            cursor.pop();
        }
        if !cursor.rest().is_empty() && !matches!(cursor.peek(), Some(' ' | '\t')) {
            return None;
        }
        civil.hour = civil.hour % 12 + if pm { 12 } else { 0 };
    } else if !has_t && !separated {
        return None;
    }
    Some(token(&cursor, civil, TIME, ProductionTag::Time))
}
fn fraction(cursor: &mut Cursor<'_>) -> Option<i64> {
    let start = cursor.position;
    let mut micros = 0;
    let mut digits = 0;
    while let Some(byte) = cursor
        .text
        .as_bytes()
        .get(cursor.position)
        .filter(|byte| byte.is_ascii_digit())
    {
        if digits < 6 {
            micros = micros * 10 + i64::from(*byte - b'0');
        }
        cursor.position += 1;
        digits += 1;
    }
    if cursor.position == start {
        return None;
    }
    for _ in digits..6 {
        micros *= 10;
    }
    Some(micros)
}
fn embedded_timestamp(text: &str) -> Option<Token> {
    let mut cursor = Cursor::new(text);
    cursor.source_literal('@')?;
    let negative = cursor.peek() == Some('-');
    if negative {
        cursor.pop();
    }
    let start = cursor.position;
    while cursor.peek().is_some_and(|c| c.is_ascii_digit()) {
        cursor.pop();
    }
    if cursor.position == start {
        return None;
    }
    let seconds = cursor
        .text
        .get(start..cursor.position)?
        .parse::<i128>()
        .ok()?;
    let micros = if cursor.peek() == Some('.') {
        cursor.pop();
        let start = cursor.position;
        let mut value = 0;
        while cursor.position - start < 6 && cursor.peek().is_some_and(|c| c.is_ascii_digit()) {
            value = value * 10 + i64::from(cursor.pop()?.to_digit(10)?);
        }
        for _ in (cursor.position - start)..6 {
            value *= 10;
        }
        value
    } else {
        0
    };
    let seconds = i64::try_from(if negative { -seconds } else { seconds }).ok()?;
    let civil = Civil {
        second: seconds,
        microsecond: if negative { -micros } else { micros },
        ..Civil::epoch()
    };
    Some(token(&cursor, civil, DATE | TIME, ProductionTag::Timestamp))
}

/// Carbon Timestamp 的字符串 chunks 相加；首符号统辖全部数字，不把 e 当指数。
pub(super) fn carbon_timestamp(text: &str) -> Result<CalendarInstant> {
    let negative = text.starts_with('-');
    let mut seconds = 0i128;
    let mut fractional = 0f64;
    for chunk in text.split(|c: char| !c.is_ascii_digit() && c != '.') {
        let mut parts = chunk.split('.');
        let integer = parts.next().unwrap_or("");
        if !integer.is_empty() {
            seconds = seconds
                .checked_add(integer.parse::<i128>().map_err(|_| range_error())?)
                .ok_or_else(range_error)?;
        }
        let decimal = parts.next().unwrap_or("");
        if !decimal.is_empty() {
            fractional += format!("0.{decimal}")
                .parse::<f64>()
                .map_err(|_| range_error())?;
        }
    }
    // 与固定 Carbon round(..., 6) 一致，限制转换区间而不截断 i64 epoch。
    let carry = fractional.floor();
    if !carry.is_finite() || carry > 20_971_520.0 {
        return Err(range_error());
    }
    #[allow(clippy::cast_possible_truncation)]
    let carry = carry as i32;
    seconds = seconds
        .checked_add(i128::from(carry))
        .ok_or_else(range_error)?;
    let fraction = ((fractional - f64::from(carry)) * 1_000_000.0).round();
    #[allow(clippy::cast_possible_truncation)]
    let micros = fraction as i64;
    // Carbon 的 floor 中间值为 float，precision14 将该量级转为指数，U u 格式因此失败。
    if seconds
        .checked_add(i128::from(micros / 1_000_000))
        .ok_or_else(range_error)?
        >= 100_000_000_000_000
    {
        return Err(range_error());
    }
    let total = seconds
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_add(i128::from(micros)))
        .ok_or_else(range_error)?;
    let total = if negative {
        total.checked_neg().ok_or_else(range_error)?
    } else {
        total
    };
    CalendarInstant::new(
        i64::try_from(total.div_euclid(1_000_000)).map_err(|_| range_error())?,
        u32::try_from(total.rem_euclid(1_000_000)).map_err(|_| range_error())?,
    )
}

/// PHP `is_numeric` 的十进制/指数字符串入口；NaN、INF 与内部空格不进入 Carbon timestamp。
pub(super) fn is_numeric(text: &str) -> bool {
    let mut cursor = Cursor::new(text);
    if matches!(cursor.peek(), Some('+' | '-')) {
        cursor.pop();
    }
    let mut digits = 0;
    while cursor.peek().is_some_and(|c| c.is_ascii_digit()) {
        cursor.pop();
        digits += 1;
    }
    if cursor.peek() == Some('.') {
        cursor.pop();
        while cursor.peek().is_some_and(|c| c.is_ascii_digit()) {
            cursor.pop();
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if matches!(cursor.peek(), Some('e' | 'E')) {
        cursor.pop();
        if matches!(cursor.peek(), Some('+' | '-')) {
            cursor.pop();
        }
        let start = cursor.position;
        while cursor.peek().is_some_and(|c| c.is_ascii_digit()) {
            cursor.pop();
        }
        if cursor.position == start {
            return false;
        }
    }
    cursor.rest().is_empty()
}

/// 默认 tz/tzcorrection 的 source prefix；显式 timezone 参数仍用完整构造合同。
pub(super) fn timezone(text: &str) -> Option<(usize, ResolvedTimezone)> {
    let mut best = None;
    let numeric_start = usize::from(text.starts_with("gmt")) * 3;
    if let Some(rest) = text.get(numeric_start..)
        && rest.starts_with(['+', '-'])
    {
        for length in 2..=9 {
            if let Some(candidate) = rest.get(..length)
                && correction(candidate)
                && let Ok(zone) = ResolvedTimezone::parse(candidate)
            {
                best = Some((numeric_start + length, zone));
            }
        }
    }
    let mut cursor = Cursor::new(text);
    let parenthesized = cursor.peek() == Some('(');
    if parenthesized {
        cursor.pop();
    }
    let start = cursor.position;
    while cursor.position - start < 6 && cursor.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
        cursor.pop();
    }
    let end = cursor.position;
    if end > start
        && let Ok(zone) = ResolvedTimezone::parse(cursor.text.get(start..end)?)
    {
        if cursor.peek() == Some(')') {
            cursor.pop();
        }
        if best
            .as_ref()
            .is_none_or(|(length, _)| cursor.position > *length)
        {
            best = Some((cursor.position, zone));
        }
    }
    let mut cursor = Cursor::new(text);
    while cursor.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
        cursor.pop();
    }
    let first = cursor.position;
    let mut segments = 0;
    while matches!(cursor.peek(), Some('_' | '/' | '-')) {
        cursor.pop();
        let start = cursor.position;
        while cursor.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            cursor.pop();
        }
        if cursor.position == start {
            break;
        }
        segments += 1;
    }
    if first >= 2
        && segments > 0
        && let Ok(zone) = ResolvedTimezone::parse(cursor.text.get(..cursor.position)?)
        && best
            .as_ref()
            .is_none_or(|(length, _)| cursor.position > *length)
    {
        best = Some((cursor.position, zone));
    }
    best
}
fn correction(text: &str) -> bool {
    let Some(body) = text.get(1..) else {
        return false;
    };
    // hour24 (':'? minute)?；1+1位可匹配+25，其构造语义仍是25小时。
    for hour_width in 1..=2 {
        let mut cursor = Cursor::new(body);
        if cursor.source_number(hour_width, hour_width, 24).is_none() {
            continue;
        }
        if cursor.rest().is_empty() {
            return true;
        }
        if cursor.peek() == Some(':') {
            cursor.pop();
        }
        for minute_width in 1..=2 {
            let mut tail = Cursor::new(cursor.rest());
            if tail.source_number(minute_width, minute_width, 59).is_some()
                && tail.rest().is_empty()
            {
                return true;
            }
        }
    }
    for colon in [false, true] {
        let mut cursor = Cursor::new(body);
        if cursor.source_number(2, 2, 24).is_none() {
            continue;
        }
        if colon && cursor.source_literal(':').is_none() {
            continue;
        }
        if cursor.source_number(2, 2, 59).is_none() {
            continue;
        }
        if colon && cursor.source_literal(':').is_none() {
            continue;
        }
        if cursor.source_number(2, 2, 60).is_some() && cursor.rest().is_empty() {
            return true;
        }
    }
    false
}

/// 固定 PHP 8.5 `number_format` 的6位 half-up；不是 Rust format 的ties-even。
/// Copyright (c) The PHP Group；移植 ext/standard/math.c，完整 PHP3.01许可见LICENSE.php。
#[allow(clippy::float_cmp)]
pub(super) fn number_format_timestamp(value: f64) -> Result<String> {
    if !value.is_finite() {
        return Err(range_error());
    }
    let exponent = 1_000_000.0;
    let mut integral = (value * exponent).trunc();
    let adjacent = integral + 1.0f64.copysign(value);
    if adjacent / exponent == value {
        integral = adjacent;
    }
    let rounded = if integral.abs() >= 1e16 {
        value
    } else {
        let edge = ((integral + 0.5f64.copysign(value)) / exponent).abs();
        if value.abs() >= edge {
            integral += 1.0f64.copysign(value);
        }
        integral / exponent
    };
    Ok(format!("{rounded:.6}"))
}
