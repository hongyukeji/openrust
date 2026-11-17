//! 保留 Carbon 原始分量的输入区间解析；不进行自动进位。

use serde_json::Value;

use crate::support::input_scalars;
use std::fmt;

/// 输入区间解析失败的安全分类，不包含用户输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalError {
    /// 区间表达式无效。
    InvalidSyntax,
    /// 指定单位无效。
    InvalidUnit,
    /// 分量超出可表示范围。
    OutOfRange,
    /// 源字符串与显式参数超过解析工作预算。
    Limit,
    /// 相对时间语法尚待完整 timelib 对应实现。
    UnsupportedRelative,
}

impl fmt::Display for IntervalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSyntax => "无效的输入区间",
            Self::InvalidUnit => "无效的区间单位",
            Self::OutOfRange => "输入区间超出范围",
            Self::Limit => "输入区间超过解析限额",
            Self::UnsupportedRelative => "尚未支持的相对区间语法",
        })
    }
}

impl std::error::Error for IntervalError {}

/// 保留年月与有符号子分量的区间，不等同于固定秒数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputInterval {
    /// 年分量。
    pub years: i64,
    /// 月分量。
    pub months: i64,
    /// 日分量（包括星期折算的七日）。
    pub days: i64,
    /// 小时分量。
    pub hours: i64,
    /// 分钟分量。
    pub minutes: i64,
    /// 秒分量。
    pub seconds: i64,
    /// 有符号微秒分量，不自动进位至秒。
    pub microseconds: i64,
    /// 独立的方向标记；负分量不会自动设置该标记。
    pub invert: bool,
    // PHP ISO/relative 构造均从 TIMELIB_UNSET 开始，本读取入口不执行日期 diff。
    total_days: Option<i64>,
}

#[derive(Clone, Copy)]
enum Unit {
    Year(i64),
    Month(i64),
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
}

impl InputInterval {
    /// 日期 diff 已知的累计天数；当前字符串/显式单位构造没有该信息，返回 None。
    ///
    /// 此值不是 days 分量，也不从年/月分量估算固定天数。
    pub const fn total_days(&self) -> Option<i64> {
        self.total_days
    }
    /// 从唯一输入 bag 已解析的值读取区间。
    ///
    /// 缺失、null、空白字符串返回 `None`；未指定单位的非字符串同样返回
    /// `None`。显式单位使用 PHP 数值转换，和 human 表达式的单位别名分开。
    pub fn parse(value: Option<&Value>, unit: Option<&str>) -> Result<Option<Self>, IntervalError> {
        let Some(value) = value else { return Ok(None) };
        // 值可由程序 merge 注入；bag 的展开键预算不能限制源字符串长度。
        let source_bytes = value.as_str().map_or(0, str::len);
        if source_bytes
            .checked_add(unit.map_or(0, str::len))
            .is_none_or(|bytes| bytes > super::MAX_CALENDAR_BYTES)
        {
            return Err(IntervalError::Limit);
        }
        if value.is_null()
            || value
                .as_str()
                .is_some_and(|s| s.trim_matches(php_empty_space).is_empty())
        {
            return Ok(None);
        }
        if let Some(unit) = unit {
            let unit = explicit_unit(unit)?;
            let value = input_scalars::float(value);
            if !value.is_finite() {
                // number_format(INF, 10) 得到 inf；fromString 没有数字片段，返回零区间。
                return Ok(Some(Self::default()));
            }
            // 先按 10 位小数格式化，再交给 Carbon。
            // 先作十位 half-up，再转定点，避免 Rust ties-even 格式化改变中点。
            let formatted = format!("{:.10}", php_round(value, 10_000_000_000.0));
            let mut result = Self::default();
            result.add_text(&formatted, unit)?;
            return Ok(Some(result));
        }
        let Some(text) = value.as_str() else {
            return Ok(None);
        };
        let text = text.trim_matches(php_empty_space);
        if text.starts_with('P')
            && text
                .as_bytes()
                .get(1)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'T')
        {
            return Self::iso(text).map(Some);
        }
        Self::relative(text)
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "固定 PHP 分数计算先把整数转回 double 再相减"
    )]
    fn add(&mut self, value: f64, unit: Unit) -> Result<(), IntervalError> {
        let integer = checked_integer(value)?;
        self.add_parts(integer, value - integer as f64, unit)
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "保留 PHP numeric-string 整数与 double 分数的不同转换路径"
    )]
    fn add_text(&mut self, text: &str, unit: Unit) -> Result<(), IntervalError> {
        let value = text.parse::<f64>().map_err(|_| IntervalError::OutOfRange)?;
        if !value.is_finite() {
            // PHP 非有限 numeric-string 转 int 为0；其分数在末级转换同样归零。
            return Ok(());
        }
        let integer = input_scalars::integer(&Value::String(text.to_owned()));
        self.add_parts(integer, value - integer as f64, unit)
    }

    fn add_parts(
        &mut self,
        mut integer: i64,
        mut fraction: f64,
        unit: Unit,
    ) -> Result<(), IntervalError> {
        if !fraction.is_finite() {
            return Err(IntervalError::OutOfRange);
        }
        // Carbon fromString 只用六位 rounding 修正接近零/一的误差，不先量化其他分数。
        match php_round(fraction, 1_000_000.0) {
            1.0 => {
                integer = integer.checked_add(1).ok_or(IntervalError::OutOfRange)?;
                fraction = 0.0;
            }
            0.0 => fraction = 0.0,
            _ => {}
        }
        match unit {
            Unit::Year(factor) => increment(&mut self.years, integer, factor),
            Unit::Month(factor) => increment(&mut self.months, integer, factor),
            Unit::Week => {
                increment(&mut self.days, integer, 7)?;
                self.add(fraction * 7.0, Unit::Day)
            }
            Unit::Day => {
                increment(&mut self.days, integer, 1)?;
                self.add(fraction * 24.0, Unit::Hour)
            }
            Unit::Hour => {
                increment(&mut self.hours, integer, 1)?;
                self.add(fraction * 60.0, Unit::Minute)
            }
            Unit::Minute => {
                increment(&mut self.minutes, integer, 1)?;
                self.add(fraction * 60.0, Unit::Second)
            }
            Unit::Second => {
                increment(&mut self.seconds, integer, 1)?;
                self.add(fraction * 1000.0, Unit::Millisecond)
            }
            Unit::Millisecond => {
                increment(&mut self.microseconds, integer, 1000)?;
                increment(
                    &mut self.microseconds,
                    checked_integer(php_round(fraction * 1000.0, 1.0))?,
                    1,
                )
            }
            Unit::Microsecond => increment(&mut self.microseconds, integer, 1),
        }
    }

    fn negate(&mut self) -> Result<(), IntervalError> {
        for value in [
            &mut self.years,
            &mut self.months,
            &mut self.days,
            &mut self.hours,
            &mut self.minutes,
            &mut self.seconds,
            &mut self.microseconds,
        ] {
            *value = value.checked_neg().ok_or(IntervalError::OutOfRange)?;
        }
        Ok(())
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "复现 Carbon 构造器以 double 计算小数修正量的路径"
    )]
    fn iso(text: &str) -> Result<Self, IntervalError> {
        if text.as_bytes().get(5) == Some(&b'-') {
            return Self::iso_alternative(text);
        }
        let mut result = Self::default();
        let mut rest = &text[1..];
        let mut time = false;
        let mut previous = 0;
        let mut count = 0;
        while !rest.is_empty() {
            if rest.starts_with('T') && !time {
                time = true;
                rest = &rest[1..];
                if rest.is_empty() {
                    return Err(IntervalError::InvalidSyntax);
                }
            }
            let (number, tail) = iso_number_prefix(rest)?;
            let symbol = tail
                .as_bytes()
                .first()
                .copied()
                .ok_or(IntervalError::InvalidSyntax)?;
            let (rank, unit) = match (time, symbol) {
                (false, b'Y') => (1, Unit::Year(1)),
                (false, b'M') => (2, Unit::Month(1)),
                (false, b'W') => (3, Unit::Week),
                (false, b'D') => (4, Unit::Day),
                (true, b'H') => (5, Unit::Hour),
                (true, b'M') => (6, Unit::Minute),
                (true, b'S') => (7, Unit::Second),
                _ => return Err(IntervalError::InvalidSyntax),
            };
            if rank <= previous {
                return Err(IntervalError::InvalidSyntax);
            }
            previous = rank;
            count += 1;
            if rank == 7 {
                // ISO 秒的小数由 Carbon 构造器截取六位，而 human 秒按微秒舍入。
                let raw = &rest[..rest.len() - tail.len()];
                result.seconds = checked_integer(number)?;
                if let Some((_, digits)) = raw.split_once('.') {
                    if digits
                        .as_bytes()
                        .get(6..)
                        .is_some_and(|tail| tail.iter().any(|b| *b != b'0'))
                    {
                        return Err(IntervalError::InvalidSyntax);
                    }
                    let mut micros = 0_i64;
                    for position in 0..6 {
                        micros = micros * 10
                            + i64::from(digits.as_bytes().get(position).map_or(0, |b| b - b'0'));
                    }
                    result.microseconds = micros;
                }
            } else {
                let component = if matches!(unit, Unit::Week) {
                    number * 7.0
                } else {
                    number
                };
                let delta = component - checked_integer(component)? as f64;
                if delta != 0.0 && input_scalars::string(Value::from(delta)).contains('E') {
                    // Carbon 构造器把修正量按 PHP precision14 字符串送进 fromString；
                    // 科学记数法会被当作无效单位，而不是当作十进制小数接受。
                    return Err(IntervalError::InvalidSyntax);
                }
                result.add(number, unit)?;
                // ISO 非秒小数经 Carbon::add 合并时不保留亚秒；human 不走该路径。
                result.microseconds = 0;
            }
            rest = &tail[1..];
        }
        if count == 0 {
            Err(IntervalError::InvalidSyntax)
        } else {
            Ok(result)
        }
    }

    fn iso_alternative(text: &str) -> Result<Self, IntervalError> {
        let bytes = text.as_bytes();
        if bytes.len() != 20
            || bytes[5] != b'-'
            || bytes[8] != b'-'
            || bytes[11] != b'T'
            || bytes[14] != b':'
            || bytes[17] != b':'
        {
            return Err(IntervalError::InvalidSyntax);
        }
        let mut values = [0_i64; 6];
        for (index, (start, end)) in [(1, 5), (6, 8), (9, 11), (12, 14), (15, 17), (18, 20)]
            .into_iter()
            .enumerate()
        {
            let part = text.get(start..end).ok_or(IntervalError::InvalidSyntax)?;
            if !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(IntervalError::InvalidSyntax);
            }
            values[index] = part.parse().map_err(|_| IntervalError::OutOfRange)?;
        }
        Ok(Self {
            years: values[0],
            months: values[1],
            days: values[2],
            hours: values[3],
            minutes: values[4],
            seconds: values[5],
            ..Self::default()
        })
    }

    fn relative(text: &str) -> Result<Option<Self>, IntervalError> {
        // Carbon 的非紧凑表达式委托 timelib；这些标记只影响相对状态，不产生分量。
        let mut budget = TermBudget::default();
        let (from_string, normalized) = relative_source(text, &mut budget)?;
        let mut rest = normalized.trim_matches(human_space);
        let mut result = Self::default();
        let mut matched = false;
        let mut terms = 0;
        while !rest.is_empty() {
            rest = rest.trim_start_matches(human_space);
            if rest.is_empty() {
                break;
            }
            terms += 1;
            budget.observe(terms)?;
            if let Some(tail) = strip_instruction(rest, "first day of")
                .or_else(|| strip_instruction(rest, "last day of"))
            {
                rest = tail;
                continue;
            }
            if let Some(tail) = rest.strip_prefix("ago")
                && (tail.is_empty() || tail.starts_with(human_space))
            {
                result.negate()?;
                rest = tail;
                continue;
            }
            if rest == "now" {
                break;
            }
            if let Some((days, tail)) = relative_day_prefix(rest) {
                increment(&mut result.days, days, 1)?;
                matched = true;
                rest = tail;
                continue;
            }
            let direction = direction_prefix(rest);
            if let Some((_, tail)) = direction {
                rest = tail;
            }
            // timelib 的 weekday 状态不属于普通 interval 分量。
            if direction.is_none() && rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
                let end = rest.find(human_space).unwrap_or(rest.len());
                if is_weekday(&rest[..end]) {
                    rest = &rest[end..];
                    continue;
                }
            }
            let (number, name, tail) = relative_term(rest, direction.map(|(number, _)| number))?;
            if (is_weekday(name) || matches!(name, "weekday" | "weekdays")) && direction.is_some() {
                if is_weekday(name) && number > 1.0 {
                    increment(&mut result.days, checked_integer(number - 1.0)?, 7)?;
                    matched = true;
                }
                // timelib 的 weekdayof 只允许带 ordinal/direction 的完整 weekday 指令。
                let tail = tail.trim_start_matches(human_space);
                rest = if is_weekday(name) {
                    strip_instruction(tail, "of").unwrap_or(tail)
                } else {
                    tail
                };
                continue;
            }
            if !from_string && matches!(name, "weekday" | "weekdays") {
                rest = tail;
                continue;
            }
            let fortnight = !from_string
                && matches!(
                    name,
                    "fortnight" | "fortnights" | "forthnight" | "forthnights"
                );
            let (unit, number) = if fortnight {
                (Unit::Week, number * 2.0)
            } else {
                let unit = if from_string {
                    human_unit(name)
                } else {
                    fallback_unit(name)
                };
                (unit.ok_or(IntervalError::InvalidSyntax)?, number)
            };
            add_relative_number(
                &mut result,
                direction.is_none().then_some(rest),
                number,
                unit,
                fortnight,
            )?;
            matched = true;
            rest = tail;
            if rest.starts_with('+') {
                rest = &rest[1..];
            }
            // 未消费的 timelib 指令需由后续语法实现，不能当成功结果忽略。
            if !rest.is_empty()
                && !rest.starts_with(|c: char| {
                    human_space(c) || c.is_ascii_digit() || c == '-' || c == '+'
                })
            {
                return Err(IntervalError::UnsupportedRelative);
            }
        }
        Ok((matched && (from_string || result != Self::default())).then_some(result))
    }
}

#[derive(Default)]
struct TermBudget {
    observed: usize,
}
impl TermBudget {
    fn observe(&mut self, logical_term: usize) -> Result<(), IntervalError> {
        // 分类与消费会读取同一项两遍；共享逻辑项高水位，避免把1024项误计成2048。
        self.observed = self.observed.max(logical_term);
        if self.observed > 1024 {
            Err(IntervalError::Limit)
        } else {
            Ok(())
        }
    }
}
fn strip_instruction<'a>(source: &'a str, instruction: &str) -> Option<&'a str> {
    let tail = source.strip_prefix(instruction)?;
    (tail.is_empty() || tail.starts_with(human_space)).then_some(tail)
}

fn relative_source(text: &str, budget: &mut TermBudget) -> Result<(bool, String), IntervalError> {
    let from_string = compact_definition(text, budget)?;
    if !from_string && text.contains('.') {
        return Err(IntervalError::InvalidSyntax);
    }
    Ok((
        from_string,
        text.to_ascii_lowercase()
            .replace(',', " ")
            .replace(" and ", " "),
    ))
}

fn add_relative_number(
    result: &mut InputInterval,
    source: Option<&str>,
    amount: f64,
    unit: Unit,
    fortnight: bool,
) -> Result<(), IntervalError> {
    if let Some(text) = source {
        let (_, tail) = number_prefix(text, true)?;
        let raw = &text[..text.len() - tail.len()];
        result.add_text(raw, unit)?;
        if fortnight {
            result.add_text(raw, unit)?;
        }
        Ok(())
    } else {
        result.add(amount, unit)
    }
}

// Carbon::makeFromString 先按这个紧凑 grammar 选择 fromString；
// 不满足时才委托 timelib，二者的别名、零区间和小数行为不同。
fn compact_definition(mut text: &str, budget: &mut TermBudget) -> Result<bool, IntervalError> {
    let mut terms = 0;
    while !text.is_empty() {
        text = text.trim_start_matches(human_space);
        if text.is_empty() {
            return Ok(false);
        }
        terms += 1;
        budget.observe(terms)?;
        if text.starts_with('+') {
            return Ok(false);
        }
        let Ok((_, tail)) = number_prefix(text, true) else {
            return Ok(false);
        };
        let tail = tail.trim_start_matches(human_space);
        let length = tail.bytes().take_while(u8::is_ascii_alphabetic).count();
        if length == 0 {
            return Ok(false);
        }
        text = &tail[length..];
    }
    Ok(true)
}

fn fallback_unit(name: &str) -> Option<Unit> {
    Some(match name {
        "year" | "years" => Unit::Year(1),
        "month" | "months" => Unit::Month(1),
        "week" | "weeks" => Unit::Week,
        "day" | "days" => Unit::Day,
        "hour" | "hours" => Unit::Hour,
        "min" | "mins" | "minute" | "minutes" => Unit::Minute,
        "sec" | "secs" | "second" | "seconds" => Unit::Second,
        "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => Unit::Millisecond,
        "µs" | "usec" | "usecs" | "µsec" | "µsecs" | "microsecond" | "microseconds" => {
            Unit::Microsecond
        }
        _ => return None,
    })
}

fn relative_term(text: &str, direction: Option<f64>) -> Result<(f64, &str, &str), IntervalError> {
    let (number, tail) = if let Some(number) = direction {
        (number, text)
    } else {
        number_prefix(text, true)?
    };
    let tail = tail.trim_start_matches(human_space);
    let length = tail
        .char_indices()
        .take_while(|(_, c)| c.is_alphabetic() || *c == 'µ')
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    if length == 0 {
        return Err(IntervalError::InvalidSyntax);
    }
    Ok((number, &tail[..length], &tail[length..]))
}

fn direction_prefix(text: &str) -> Option<(f64, &str)> {
    for (word, amount) in [
        ("next ", 1.0),
        ("last ", -1.0),
        ("previous ", -1.0),
        ("this ", 0.0),
        ("first ", 1.0),
        ("second ", 2.0),
        ("third ", 3.0),
        ("fourth ", 4.0),
        ("fifth ", 5.0),
        ("sixth ", 6.0),
        ("seventh ", 7.0),
        ("eighth ", 8.0),
        ("ninth ", 9.0),
        ("tenth ", 10.0),
        ("eleventh ", 11.0),
        ("twelfth ", 12.0),
    ] {
        if let Some(tail) = text.strip_prefix(word) {
            return Some((amount, tail));
        }
    }
    None
}

fn relative_day_prefix(text: &str) -> Option<(i64, &str)> {
    for (word, days) in [
        ("tomorrow", 1),
        ("yesterday", -1),
        ("today", 0),
        ("midnight", 0),
    ] {
        if let Some(tail) = text.strip_prefix(word)
            && (tail.is_empty() || tail.starts_with(human_space))
        {
            return Some((days, tail));
        }
    }
    None
}

fn human_space(c: char) -> bool {
    c.is_ascii_whitespace()
        || matches!(
            c,
            '\u{00a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'
                ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
        )
}

fn php_empty_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\0')
}

// PHP 8.5 ext/standard/math.c::_php_math_round / PHP_ROUND_HALF_UP；这里只用 0/6/10 位。
// 比较原值与舍入中点，避免先乘后 round 造成可观察的二进制中点偏差。
#[allow(
    clippy::float_cmp,
    reason = "PHP half-up 算法以精确 IEEE754 比较识别可逆整数，不用 epsilon 改变边界"
)]
fn php_round(value: f64, exponent: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let mut integral = (value * exponent).trunc();
    let corrected = integral + 1.0_f64.copysign(value);
    if corrected / exponent == value {
        integral = corrected;
    }
    if integral.abs() >= 1e16 {
        return value;
    }
    let edge = ((integral + 0.5_f64.copysign(integral)) / exponent).abs();
    if value.abs() >= edge {
        integral += 1.0_f64.copysign(integral);
    }
    integral / exponent
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "已检查 i64 范围后按 PHP 截去小数部分"
)]
fn checked_integer(value: f64) -> Result<i64, IntervalError> {
    if !value.is_finite()
        || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
    {
        return Err(IntervalError::OutOfRange);
    }
    Ok(value.trunc() as i64)
}

fn increment(target: &mut i64, value: i64, factor: i64) -> Result<(), IntervalError> {
    *target = target
        .checked_add(value.checked_mul(factor).ok_or(IntervalError::OutOfRange)?)
        .ok_or(IntervalError::OutOfRange)?;
    Ok(())
}

fn iso_number_prefix(text: &str) -> Result<(f64, &str), IntervalError> {
    let bytes = text.as_bytes();
    let mut end = usize::from(bytes.first().is_some_and(|b| matches!(b, b'+' | b'-')));
    let mut digits = 0;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
        digits += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
            digits += 1;
        }
        if end == start {
            return Err(IntervalError::InvalidSyntax);
        }
    }
    if digits == 0 {
        return Err(IntervalError::InvalidSyntax);
    }
    let number = text[..end]
        .parse::<f64>()
        .map_err(|_| IntervalError::OutOfRange)?;
    if !number.is_finite() {
        return Err(IntervalError::OutOfRange);
    }
    Ok((number, &text[end..]))
}

fn number_prefix(text: &str, signed: bool) -> Result<(f64, &str), IntervalError> {
    let bytes = text.as_bytes();
    let mut end = usize::from(signed && bytes.first().is_some_and(|b| *b == b'-' || *b == b'+'));
    let start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == start {
        return Err(IntervalError::InvalidSyntax);
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start {
            return Err(IntervalError::InvalidSyntax);
        }
    }
    let number = text[..end]
        .parse::<f64>()
        .map_err(|_| IntervalError::OutOfRange)?;
    Ok((number, &text[end..]))
}

fn explicit_unit(name: &str) -> Result<Unit, IntervalError> {
    let name = name.to_ascii_lowercase();
    let singular = match name.as_str() {
        "centuries" => "century",
        "millennia" => "millennium",
        name => name.trim_end_matches('s'),
    };
    match singular {
        "millennium" => Ok(Unit::Year(1000)),
        "century" => Ok(Unit::Year(100)),
        "decade" => Ok(Unit::Year(10)),
        "year" => Ok(Unit::Year(1)),
        "quarter" => Ok(Unit::Month(3)),
        "month" => Ok(Unit::Month(1)),
        "week" => Ok(Unit::Week),
        "day" => Ok(Unit::Day),
        "hour" => Ok(Unit::Hour),
        "minute" => Ok(Unit::Minute),
        "second" => Ok(Unit::Second),
        "millisecond" => Ok(Unit::Millisecond),
        "microsecond" => Ok(Unit::Microsecond),
        _ => Err(IntervalError::InvalidUnit),
    }
}

fn human_unit(name: &str) -> Option<Unit> {
    Some(match name {
        "millennia" | "millennium" => Unit::Year(1000),
        "century" | "centuries" => Unit::Year(100),
        "decade" | "decades" => Unit::Year(10),
        "year" | "years" | "y" | "yr" | "yrs" => Unit::Year(1),
        "quarter" | "quarters" => Unit::Month(3),
        "month" | "months" | "mo" | "mos" => Unit::Month(1),
        "week" | "weeks" | "w" => Unit::Week,
        "day" | "days" | "d" => Unit::Day,
        "hour" | "hours" | "h" => Unit::Hour,
        "minute" | "minutes" | "m" => Unit::Minute,
        "second" | "seconds" | "s" => Unit::Second,
        "millisecond" | "milliseconds" | "milli" | "ms" => Unit::Millisecond,
        "microsecond" | "microseconds" | "micro" | "µs" => Unit::Microsecond,
        _ => return None,
    })
}

fn is_weekday(name: &str) -> bool {
    matches!(
        name,
        "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
    )
}

#[cfg(test)]
#[path = "../../../tests/unit/calendar_input_intervals.rs"]
mod tests;
