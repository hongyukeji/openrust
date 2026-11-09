//! 保留 Carbon 原始分量的输入区间解析；不进行自动进位。

use serde_json::Value;
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
            .is_none_or(|bytes| bytes > 20 * 1024 * 1024)
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
            let value = crate::support::scalars::float(value);
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
        let integer = crate::support::scalars::integer(&Value::String(text.to_owned()));
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
                if delta != 0.0
                    && crate::support::scalars::string(&Value::from(delta)).contains('E')
                {
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
#[allow(
    clippy::unwrap_used,
    reason = "固定合成 fixture 构造或断言失败直接中止当前测试"
)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parsed_intervals_do_not_invent_total_days_from_day_components() {
        for text in ["P1Y2M3D", "3 days", "1 week ago"] {
            let interval = InputInterval::parse(Some(&serde_json::json!(text)), None)
                .unwrap()
                .unwrap();
            assert_eq!(interval.total_days(), None);
        }
        let interval = InputInterval::parse(Some(&serde_json::json!(90)), Some("day"))
            .unwrap()
            .unwrap();
        assert_eq!(interval.days, 90);
        assert_eq!(interval.total_days(), None);
    }

    #[test]
    fn of_is_only_consumed_by_complete_calendar_instructions() {
        for text in [
            "of 2 days",
            "of of 2 days",
            "1 day of 2 days",
            "Monday of next month",
            "first day of of next month",
        ] {
            assert_eq!(
                InputInterval::parse(Some(&json!(text)), None),
                Err(IntervalError::InvalidSyntax)
            );
        }
        assert_eq!(
            InputInterval::parse(Some(&json!("first monday of next month")), None)
                .unwrap()
                .unwrap()
                .months,
            1
        );
        assert_eq!(
            InputInterval::parse(Some(&json!("second Tuesday of next month")), None)
                .unwrap()
                .unwrap()
                .days,
            7
        );
        assert_eq!(
            InputInterval::parse(Some(&json!("first day of 2 days")), None)
                .unwrap()
                .unwrap()
                .days,
            2
        );
    }
    #[test]
    fn one_shared_term_budget_bounds_compact_human_and_relative_consumption() {
        for term in ["0d", "0 days ", "tomorrow "] {
            assert!(InputInterval::parse(Some(&json!(term.repeat(1024))), None).is_ok());
            assert_eq!(
                InputInterval::parse(Some(&json!(term.repeat(1025))), None),
                Err(IntervalError::Limit)
            );
        }
    }

    // 固定 Carbon 3.8.4 + PHP 8.5 golden，区分 missing/null、非字符串与无单位数字字符串。
    #[test]
    fn absence_and_invalid_are_different() {
        for value in [
            Value::Null,
            json!(" \t\n\0"),
            json!(90),
            json!(false),
            json!([]),
        ] {
            assert_eq!(InputInterval::parse(Some(&value), None), Ok(None));
        }
        assert_eq!(InputInterval::parse(None, Some("bad")), Ok(None));
        for text in [
            "90",
            "0",
            "invalid",
            "P1X",
            "-P1D",
            "p1d",
            "P-1D",
            "2 weekdays",
        ] {
            assert!(
                InputInterval::parse(Some(&json!(text)), None).is_err(),
                "{text}"
            );
        }
        assert_eq!(
            InputInterval::parse(Some(&json!("0 days")), None),
            Ok(Some(InputInterval::default()))
        );
    }

    // 发现错误 cascade、负数方向和年月小数被错误换算的缺陷。
    #[test]
    fn original_components_and_fraction_rules() {
        for (text, expected) in [
            (
                "2hours30minutes",
                InputInterval {
                    hours: 2,
                    minutes: 30,
                    ..InputInterval::default()
                },
            ),
            (
                "2 hours -90 minutes",
                InputInterval {
                    hours: 2,
                    minutes: -90,
                    ..InputInterval::default()
                },
            ),
            (
                "-1.5 hours",
                InputInterval {
                    hours: -1,
                    minutes: -30,
                    ..InputInterval::default()
                },
            ),
            (
                "-0.5 weeks",
                InputInterval {
                    days: -3,
                    hours: -12,
                    ..InputInterval::default()
                },
            ),
            (
                "1.5 months",
                InputInterval {
                    months: 1,
                    ..InputInterval::default()
                },
            ),
            (
                "-1.5 microseconds",
                InputInterval {
                    microseconds: -1,
                    ..InputInterval::default()
                },
            ),
            (
                "3 µs",
                InputInterval {
                    microseconds: 3,
                    ..InputInterval::default()
                },
            ),
        ] {
            assert_eq!(
                InputInterval::parse(Some(&json!(text)), None),
                Ok(Some(expected)),
                "{text}"
            );
        }
    }

    // 发现显式单位误用 human 别名、数组数值转换以及十位舍入缺陷。
    #[test]
    fn explicit_units_cast_and_decimal_precision() {
        for unit in ["m", "ms", "micro", "min", " hour ", ""] {
            assert_eq!(
                InputInterval::parse(Some(&json!(1)), Some(unit)),
                Err(IntervalError::InvalidUnit)
            );
        }
        for (value, unit, expected) in [
            (
                json!([1]),
                "hour",
                InputInterval {
                    hours: 1,
                    ..InputInterval::default()
                },
            ),
            (
                json!("1ab"),
                "HOURS",
                InputInterval {
                    hours: 1,
                    ..InputInterval::default()
                },
            ),
            (
                json!("0.12345678905"),
                "second",
                InputInterval {
                    microseconds: 123_457,
                    ..InputInterval::default()
                },
            ),
            (
                json!("0.99999999996"),
                "second",
                InputInterval {
                    seconds: 1,
                    ..InputInterval::default()
                },
            ),
            (
                json!("-0.000001"),
                "second",
                InputInterval {
                    microseconds: -1,
                    ..InputInterval::default()
                },
            ),
            (json!("0.000053"), "millisecond", InputInterval::default()),
        ] {
            assert_eq!(
                InputInterval::parse(Some(&value), Some(unit)),
                Ok(Some(expected))
            );
        }
    }

    // 发现 ISO 的分量保留、秒截断和替代日期格式被拒绝的缺陷。
    #[test]
    fn iso_and_relative_components() {
        for (text, expected) in [
            (
                "P1Y2M3DT4H5M6S",
                InputInterval {
                    years: 1,
                    months: 2,
                    days: 3,
                    hours: 4,
                    minutes: 5,
                    seconds: 6,
                    ..InputInterval::default()
                },
            ),
            (
                "P0001-02-03T04:05:06",
                InputInterval {
                    years: 1,
                    months: 2,
                    days: 3,
                    hours: 4,
                    minutes: 5,
                    seconds: 6,
                    ..InputInterval::default()
                },
            ),
            (
                "PT90M",
                InputInterval {
                    minutes: 90,
                    ..InputInterval::default()
                },
            ),
            (
                "P0.5W",
                InputInterval {
                    days: 3,
                    hours: 12,
                    ..InputInterval::default()
                },
            ),
            (
                "last day of next month",
                InputInterval {
                    months: 1,
                    ..InputInterval::default()
                },
            ),
            (
                "next Monday +2 days",
                InputInterval {
                    days: 2,
                    ..InputInterval::default()
                },
            ),
            (
                "2 days ago",
                InputInterval {
                    days: -2,
                    ..InputInterval::default()
                },
            ),
        ] {
            assert_eq!(
                InputInterval::parse(Some(&json!(text)), None),
                Ok(Some(expected)),
                "{text}"
            );
        }
        for text in ["now", "next Monday"] {
            assert_eq!(InputInterval::parse(Some(&json!(text)), None), Ok(None));
        }
    }

    // 发现整数越界通过饱和转换被静默接收的缺陷。
    #[test]
    fn component_overflow_is_checked() {
        for text in [
            "P9223372036854775808D",
            "9000000000000000000 weeks",
            "9000000000000000000 years 9000000000000000000 years",
        ] {
            assert_eq!(
                InputInterval::parse(Some(&json!(text)), None),
                Err(IntervalError::OutOfRange)
            );
        }
    }
    // 程序注入的值不受 HTTP body 限额保护，解析预算必须独立检查。
    #[test]
    fn value_and_parameter_work_budget() {
        let value = json!("x".repeat(20 * 1024 * 1024));
        assert_eq!(
            InputInterval::parse(Some(&value), Some("s")),
            Err(IntervalError::Limit)
        );
    }

    // 真实补充 probe：ISO 秒超出微秒非零精度会失败，其他单位丢弃亚秒。
    #[test]
    fn iso_precision_and_relative_supplement() {
        for text in ["PT0.1234561S", "PT0.0000001S"] {
            assert_eq!(
                InputInterval::parse(Some(&json!(text)), None),
                Err(IntervalError::InvalidSyntax)
            );
        }
        for (text, expected) in [
            (
                "PT0.1234560S",
                InputInterval {
                    microseconds: 123_456,
                    ..InputInterval::default()
                },
            ),
            (
                "P0.1234567D",
                InputInterval {
                    hours: 2,
                    minutes: 57,
                    seconds: 46,
                    ..InputInterval::default()
                },
            ),
            (
                "P1DT-1H",
                InputInterval {
                    days: 1,
                    hours: -1,
                    ..InputInterval::default()
                },
            ),
            (
                "PT-0.5S",
                InputInterval {
                    microseconds: 500_000,
                    ..InputInterval::default()
                },
            ),
            (
                "tomorrow",
                InputInterval {
                    days: 1,
                    ..InputInterval::default()
                },
            ),
            (
                "Monday next week",
                InputInterval {
                    days: 7,
                    ..InputInterval::default()
                },
            ),
            (
                "second Tuesday of next month",
                InputInterval {
                    months: 1,
                    days: 7,
                    ..InputInterval::default()
                },
            ),
            (
                "next fortnight",
                InputInterval {
                    days: 14,
                    ..InputInterval::default()
                },
            ),
            (
                "yesterday 2 days ago",
                InputInterval {
                    days: -1,
                    ..InputInterval::default()
                },
            ),
            (
                "2 days ago 3 hours",
                InputInterval {
                    days: -2,
                    hours: 3,
                    ..InputInterval::default()
                },
            ),
        ] {
            assert_eq!(
                InputInterval::parse(Some(&json!(text)), None),
                Ok(Some(expected)),
                "{text}"
            );
        }
    }

    // PHP 8.5.0 + Carbon 3.8.4 的149原始 interval 行及追加真实探针，捕获类型/分流/精度偏差。
    #[test]
    fn actual_php_carbon_golden_matrix() {
        let cases: Value = serde_json::from_str(r#"[["upstream.interval.null",null,null,0,null],["upstream.interval.empty","",null,0,null],["upstream.interval.missing","__MISSING__",null,0,null],["upstream.interval.iso","P1Y2M3DT4H5M6S",null,1,[1,2,3,4,5,6,0]],["upstream.interval.human","2 hours 30 minutes",null,1,[0,0,0,2,30,0,0]],["upstream.interval.90seconds","90","second",1,[0,0,0,0,0,90,0]],["upstream.interval.90minutes","90","minute",1,[0,0,0,0,90,0,0]],["upstream.interval.90hours","90","hour",1,[0,0,0,90,0,0,0]],["upstream.interval.90days","90","day",1,[0,0,90,0,0,0,0]],["upstream.interval.45_unit_minute","45","Minute",1,[0,0,0,0,45,0,0]],["upstream.interval.45_unit_second","45","Second",1,[0,0,0,0,0,45,0]],["upstream.interval.small_ms","0.000053","Millisecond",1,[0,0,0,0,0,0,0]],["upstream.interval.very_small_sec","0.000001","Second",1,[0,0,0,0,0,0,1]],["upstream.interval.scientific","5.3E-5","Millisecond",1,[0,0,0,0,0,0,0]],["upstream.interval.fractional_hour","1.5","Hour",1,[0,0,0,1,30,0,0]],["upstream.interval.integer_minute","90","Minute",1,[0,0,0,0,90,0,0]],["interval.make_numeric",90,null,0,null],["interval.make_numeric_string","90",null,2,null],["interval.make_true",true,null,0,null],["interval.make_array",[],null,0,null],["interval.make_nonduration","2020-01-01",null,2,null],["interval.make_relative_weekday","next Monday",null,0,null],["interval.make_now","now",null,0,null],["interval.make_invalid","invalid",null,2,null],["interval.make_zero_human","0 days",null,1,[0,0,0,0,0,0,0]],["interval.make_zero_iso","PT0S",null,1,[0,0,0,0,0,0,0]],["interval.human_compact","2hours30minutes",null,1,[0,0,0,2,30,0,0]],["interval.human_commas","2 hours, 30 minutes",null,1,[0,0,0,2,30,0,0]],["interval.human_and","2 hours and 30 minutes",null,1,[0,0,0,2,30,0,0]],["interval.negative_hour","-1.5 hours",null,1,[0,0,0,-1,-30,0,0]],["interval.negative_day","-2 days",null,1,[0,0,-2,0,0,0,0]],["interval.mixed_sign","2 hours -90 minutes",null,1,[0,0,0,2,-90,0,0]],["interval.fractional_week","0.5 weeks",null,1,[0,0,3,12,0,0,0]],["interval.fractional_month","1.5 months",null,1,[0,1,0,0,0,0,0]],["interval.fractional_year","1.5 years",null,1,[1,0,0,0,0,0,0]],["interval.decimal_10","0.12345678905","second",1,[0,0,0,0,0,0,123457]],["interval.decimal_very_tiny","0.00000000004","second",1,[0,0,0,0,0,0,0]],["interval.decimal_round_up","0.99999999996","second",1,[0,0,0,0,0,1,0]],["interval.decimal_negative_tiny","-0.000001","second",1,[0,0,0,0,0,0,-1]],["interval.numeric_prefix_unit","1ab","hour",1,[0,0,0,1,0,0,0]],["interval.nonnumeric_unit","bad","hour",1,[0,0,0,0,0,0,0]],["interval.invalid_unit","90","m",2,null],["interval.unit_plural","1.5","HOURS",1,[0,0,0,1,30,0,0]],["interval.iso_week","P2W",null,1,[0,0,14,0,0,0,0]],["interval.iso_overflow","PT90M",null,1,[0,0,0,0,90,0,0]],["interval.iso_fraction","PT0.5S",null,1,[0,0,0,0,0,0,500000]],["interval.iso_negative","-P1D",null,2,null],["interval.iso_invalid","P1X",null,2,null],["interval.unit_empty","90","",2,null],["interval.micro_alias","3 micro",null,1,[0,0,0,0,0,0,3]],["interval.micro_symbol","3 µs",null,1,[0,0,0,0,0,0,3]],["interval.milli_alias","3 milli",null,1,[0,0,0,0,0,0,3000]],["interval.bare_number_from_make","0",null,2,null],["interval.missing_invalid_unit","__MISSING__","bad",0,null],["interval.numeric_false_unit",false,"hour",1,[0,0,0,0,0,0,0]],["interval.numeric_array_unit",[1],"hour",1,[0,0,0,1,0,0,0]],["interval.make_relative_days","2 days ago",null,1,[0,0,-2,0,0,0,0]],["interval.make_relative_weekdays","2 weekdays",null,2,null],["interval.make_relative_month","next month",null,1,[0,1,0,0,0,0,0]],["interval.make_relative_month_days","last day of next month",null,1,[0,1,0,0,0,0,0]],["interval.make_relative_mix","next Monday +2 days",null,1,[0,0,2,0,0,0,0]],["interval.make_relative_noon","noon",null,2,null],["interval.iso_fraction_year","P1.5Y",null,1,[1,0,0,0,0,0,0]],["interval.iso_fraction_month","P1.5M",null,1,[0,1,0,0,0,0,0]],["interval.iso_fraction_day","P0.5D",null,1,[0,0,0,12,0,0,0]],["interval.iso_fraction_week","P0.5W",null,1,[0,0,3,12,0,0,0]],["interval.iso_duration_alt","P0001-02-03T04:05:06",null,1,[1,2,3,4,5,6,0]],["interval.iso_unsupported_lowercase","p1d",null,2,null],["interval.iso_negative_component","P-1D",null,2,null],["interval.negative_micro","-1.5 microseconds",null,1,[0,0,0,0,0,0,-1]],["interval.micro_round","1.9 microseconds",null,1,[0,0,0,0,0,0,1]],["interval.negative_week","-0.5 weeks",null,1,[0,0,-3,-12,0,0,0]],["interval.human_unknown","2 elephants",null,2,null],["interval.unit_centuries","1.5","centuries",1,[100,0,0,0,0,0,0]],["interval.unit_alias_ms","1.5","ms",2,null],["interval.unit_space","1.5"," hour ",2,null],["unit_parameter_alias.microseconds","2","microseconds",1,[0,0,0,0,0,0,2]],["unit_parameter_alias.milliseconds","2","milliseconds",1,[0,0,0,0,0,0,2000]],["unit_parameter_alias.seconds","2","seconds",1,[0,0,0,0,0,2,0]],["unit_parameter_alias.minutes","2","minutes",1,[0,0,0,0,2,0,0]],["unit_parameter_alias.hours","2","hours",1,[0,0,0,2,0,0,0]],["unit_parameter_alias.days","2","days",1,[0,0,2,0,0,0,0]],["unit_parameter_alias.weeks","2","weeks",1,[0,0,14,0,0,0,0]],["unit_parameter_alias.months","2","months",1,[0,2,0,0,0,0,0]],["unit_parameter_alias.quarters","2","quarters",1,[0,6,0,0,0,0,0]],["unit_parameter_alias.years","2","years",1,[2,0,0,0,0,0,0]],["unit_parameter_alias.decades","2","decades",1,[20,0,0,0,0,0,0]],["unit_parameter_alias.centuries","2","centuries",1,[200,0,0,0,0,0,0]],["unit_parameter_alias.millennia","2","millennia",1,[2000,0,0,0,0,0,0]],["unit_parameter_alias.micro","2","micro",2,null],["unit_parameter_alias.milli","2","milli",2,null],["unit_parameter_alias.sec","2","sec",2,null],["unit_parameter_alias.min","2","min",2,null],["unit_parameter_alias.yr","2","yr",2,null],["unit_parameter_alias.mo","2","mo",2,null],["unit_parameter_alias.µs","2","µs",2,null],["interval.unit.microsecond","1.5","Microsecond",1,[0,0,0,0,0,0,1]],["interval.unit.millisecond","1.5","Millisecond",1,[0,0,0,0,0,0,1500]],["interval.unit.second","1.5","Second",1,[0,0,0,0,0,1,500000]],["interval.unit.minute","1.5","Minute",1,[0,0,0,0,1,30,0]],["interval.unit.hour","1.5","Hour",1,[0,0,0,1,30,0,0]],["interval.unit.day","1.5","Day",1,[0,0,1,12,0,0,0]],["interval.unit.week","1.5","Week",1,[0,0,10,12,0,0,0]],["interval.unit.month","1.5","Month",1,[0,1,0,0,0,0,0]],["interval.unit.quarter","1.5","Quarter",1,[0,3,0,0,0,0,0]],["interval.unit.year","1.5","Year",1,[1,0,0,0,0,0,0]],["interval.unit.decade","1.5","Decade",1,[10,0,0,0,0,0,0]],["interval.unit.century","1.5","Century",1,[100,0,0,0,0,0,0]],["interval.unit.millennium","1.5","Millennium",1,[1000,0,0,0,0,0,0]],["interval.alias.millennia","2 millennia",null,1,[2000,0,0,0,0,0,0]],["interval.alias.millennium","2 millennium",null,1,[2000,0,0,0,0,0,0]],["interval.alias.century","2 century",null,1,[200,0,0,0,0,0,0]],["interval.alias.centuries","2 centuries",null,1,[200,0,0,0,0,0,0]],["interval.alias.decade","2 decade",null,1,[20,0,0,0,0,0,0]],["interval.alias.decades","2 decades",null,1,[20,0,0,0,0,0,0]],["interval.alias.year","2 year",null,1,[2,0,0,0,0,0,0]],["interval.alias.years","2 years",null,1,[2,0,0,0,0,0,0]],["interval.alias.y","2 y",null,1,[2,0,0,0,0,0,0]],["interval.alias.yr","2 yr",null,1,[2,0,0,0,0,0,0]],["interval.alias.yrs","2 yrs",null,1,[2,0,0,0,0,0,0]],["interval.alias.quarter","2 quarter",null,1,[0,6,0,0,0,0,0]],["interval.alias.quarters","2 quarters",null,1,[0,6,0,0,0,0,0]],["interval.alias.month","2 month",null,1,[0,2,0,0,0,0,0]],["interval.alias.months","2 months",null,1,[0,2,0,0,0,0,0]],["interval.alias.mo","2 mo",null,1,[0,2,0,0,0,0,0]],["interval.alias.mos","2 mos",null,1,[0,2,0,0,0,0,0]],["interval.alias.week","2 week",null,1,[0,0,14,0,0,0,0]],["interval.alias.weeks","2 weeks",null,1,[0,0,14,0,0,0,0]],["interval.alias.w","2 w",null,1,[0,0,14,0,0,0,0]],["interval.alias.day","2 day",null,1,[0,0,2,0,0,0,0]],["interval.alias.days","2 days",null,1,[0,0,2,0,0,0,0]],["interval.alias.d","2 d",null,1,[0,0,2,0,0,0,0]],["interval.alias.hour","2 hour",null,1,[0,0,0,2,0,0,0]],["interval.alias.hours","2 hours",null,1,[0,0,0,2,0,0,0]],["interval.alias.h","2 h",null,1,[0,0,0,2,0,0,0]],["interval.alias.minute","2 minute",null,1,[0,0,0,0,2,0,0]],["interval.alias.minutes","2 minutes",null,1,[0,0,0,0,2,0,0]],["interval.alias.m","2 m",null,1,[0,0,0,0,2,0,0]],["interval.alias.second","2 second",null,1,[0,0,0,0,0,2,0]],["interval.alias.seconds","2 seconds",null,1,[0,0,0,0,0,2,0]],["interval.alias.s","2 s",null,1,[0,0,0,0,0,2,0]],["interval.alias.millisecond","2 millisecond",null,1,[0,0,0,0,0,0,2000]],["interval.alias.milliseconds","2 milliseconds",null,1,[0,0,0,0,0,0,2000]],["interval.alias.milli","2 milli",null,1,[0,0,0,0,0,0,2000]],["interval.alias.ms","2 ms",null,1,[0,0,0,0,0,0,2000]],["interval.alias.microsecond","2 microsecond",null,1,[0,0,0,0,0,0,2]],["interval.alias.microseconds","2 microseconds",null,1,[0,0,0,0,0,0,2]],["interval.alias.micro","2 micro",null,1,[0,0,0,0,0,0,2]],["interval.alias.µs","2 µs",null,1,[0,0,0,0,0,0,2]],["additional.integer-probe.0","9007199254740993 days",null,1,[0,0,9007199254740993,0,0,0,0]],["additional.integer-probe.1","9223372036854775807 days",null,1,[0,0,9223372036854775807,0,0,0,0]],["additional.integer-probe.2","9223372036854775808 days",null,1,[0,0,9223372036854775807,0,0,0,0]],["additional.integer-probe.3","-9223372036854775808 days",null,1,[0,0,-9223372036854775808,0,0,0,0]],["additional.integer-probe.4","9007199254740993 years",null,1,[9007199254740993,0,0,0,0,0,0]],["additional.integer-probe.5","P9007199254740993D",null,1,[0,0,9007199254740992,0,0,0,0]],["additional.integer-probe.6","P9223372036854775807D",null,2,null],["additional.integer-probe.7","P9223372036854775808D",null,2,null],["additional.integer-probe.8","PT9007199254740993S",null,1,[0,0,0,0,0,9007199254740992,0]],["additional.integer-probe.9","PT-1.5S",null,1,[0,0,0,0,0,-1,500000]],["additional.integer-probe.10","PT-1.25S",null,1,[0,0,0,0,0,-1,250000]],["additional.integer-probe.11","P0.00002W",null,1,[0,0,0,0,0,12,0]],["additional.dispatch-probe.12","1.5 hours, 30 minutes",null,2,null],["additional.dispatch-probe.13","1.5 hours and 30 minutes",null,2,null],["additional.dispatch-probe.14","0 days ago",null,0,null],["additional.dispatch-probe.15","0 µs",null,0,null],["additional.dispatch-probe.16","1.5 µs",null,2,null],["additional.dispatch-probe.17","2h30m",null,1,[0,0,0,2,30,0,0]],["additional.dispatch-probe.18","2 hours; 3 minutes",null,2,null],["additional.dispatch-probe.19","2 hours 30 minutes ",null,1,[0,0,0,2,30,0,0]],["additional.dispatch-probe.20"," 2 hours",null,1,[0,0,0,2,0,0,0]],["additional.dispatch-probe.21","2 hours\\n30 minutes",null,2,null],["additional.dispatch-probe.22","2 hours +30 minutes",null,1,[0,0,0,2,30,0,0]],["additional.dispatch-probe.23","2 hours 30minutes ago",null,1,[0,0,0,-2,-30,0,0]],["additional.dispatch-probe.24","2 SECONDSS",null,2,null],["additional.dispatch-probe.25","2 hours 2 hours",null,1,[0,0,0,4,0,0,0]],["additional.dispatch-probe.26","2h2h",null,1,[0,0,0,4,0,0,0]],["additional.dispatch-probe.27","1.5 HOURS",null,1,[0,0,0,1,30,0,0]],["additional.dispatch-probe.28","1.5 HOURS,30 minutes",null,2,null],["additional.iso-small.29","P0.0001D",null,1,[0,0,0,0,0,8,0]],["additional.iso-small.30","P0.00001D",null,2,null],["additional.iso-small.31","P1.0001D",null,2,null],["additional.iso-small.32","P1.0000001Y",null,2,null],["additional.iso-small.33","PT0.0001H",null,1,[0,0,0,0,0,0,0]],["additional.iso-small.34","PT0.00001H",null,2,null],["additional.iso-small.35","PT0.0001M",null,1,[0,0,0,0,0,0,0]],["additional.relative-probe.36","second Tuesday of next month",null,1,[0,1,7,0,0,0,0]],["additional.relative-probe.37","previous month",null,1,[0,-1,0,0,0,0,0]],["additional.relative-probe.38","first month",null,1,[0,1,0,0,0,0,0]],["additional.relative-probe.39","twelfth month",null,1,[0,12,0,0,0,0,0]],["additional.relative-probe.40","next fortnight",null,1,[0,0,14,0,0,0,0]],["additional.relative-probe.41","last fortnight",null,1,[0,0,-14,0,0,0,0]],["additional.relative-probe.42","next weekday",null,0,null],["additional.relative-probe.43","next weekdays",null,0,null],["additional.relative-probe.44","second day",null,1,[0,0,2,0,0,0,0]],["additional.relative-probe.45","first Monday",null,0,null],["additional.relative-probe.46","next Monday of next month",null,1,[0,1,0,0,0,0,0]],["additional.relative-probe.47","last day of this month",null,0,null],["additional.relative-probe.48","today",null,0,null],["additional.relative-probe.49","midnight",null,0,null],["additional.relative-probe.50","tomorrow +2 hours",null,1,[0,0,1,2,0,0,0]],["additional.relative-probe.51","noon",null,2,null],["additional.relative-probe.52","yesterday 2 days ago",null,1,[0,0,-1,0,0,0,0]],["additional.relative-probe.53","2 days ago 3 hours",null,1,[0,0,-2,3,0,0,0]],["additional.relative-probe.54","2 days 3 hours ago",null,1,[0,0,-2,-3,0,0,0]],["additional.iso-more.204","P0.1234567W",null,1,[0,0,0,20,44,26,0]],["additional.iso-more.205","PT0.1234567H",null,1,[0,0,0,0,7,24,0]],["additional.iso-more.206","PT0.1234567M",null,1,[0,0,0,0,0,7,0]],["additional.iso-more.207","P1DT-1H",null,1,[0,0,1,-1,0,0,0]],["additional.iso-more.208","P1DT+1H",null,1,[0,0,1,1,0,0,0]],["additional.iso-more.209","P1Y-2M",null,1,[1,-2,0,0,0,0,0]],["additional.iso-more.210","PT-1S",null,1,[0,0,0,0,0,-1,0]],["additional.iso-more.211","PT-0.5S",null,1,[0,0,0,0,0,0,500000]],["additional.iso-more.212","P1DT.5S",null,1,[0,0,1,0,0,0,500000]],["additional.iso-more.213","P1.5D2H",null,2,null],["additional.iso-more.214","P1W2D",null,1,[0,0,9,0,0,0,0]],["additional.iso-more.215","P1.0000001D",null,2,null],["additional.iso-more.216","PT0.0000001M",null,2,null],["additional.iso-more.217","P00010203T040506",null,2,null],["additional.iso-more.218","P0001-02-03",null,2,null],["additional.iso-more.219","P00010203",null,2,null],["additional.iso-probe.220","PT0.1S",null,1,[0,0,0,0,0,0,100000]],["additional.iso-probe.221","PT0.123456S",null,1,[0,0,0,0,0,0,123456]],["additional.iso-probe.222","PT0.1234560S",null,1,[0,0,0,0,0,0,123456]],["additional.iso-probe.223","PT0.1234561S",null,2,null],["additional.iso-probe.224","PT0.999999S",null,1,[0,0,0,0,0,0,999999]],["additional.iso-probe.225","PT1.5S",null,1,[0,0,0,0,0,1,500000]],["additional.iso-probe.226","PT1.123456S",null,1,[0,0,0,0,0,1,123456]],["additional.iso-probe.227","PT0.0000001S",null,2,null],["additional.iso-probe.228","P0.1234567D",null,1,[0,0,0,2,57,46,0]],["additional.iso-probe.229","P1.1234567Y",null,1,[1,0,0,0,0,0,0]],["additional.nonfinite.230","1e1000","hour",1,[0,0,0,0,0,0,0]],["additional.nonfinite.231","-1e1000","hour",1,[0,0,0,0,0,0,0]],["additional.nonfinite.232","INF","hour",1,[0,0,0,0,0,0,0]],["additional.nonfinite.233","nan","hour",1,[0,0,0,0,0,0,0]],["additional.nonfinite.234","9999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999 years",null,1,[0,0,0,0,0,0,0]],["additional.nonfinite.235","9999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999 months",null,1,[0,0,0,0,0,0,0]],["additional.nonfinite.236","9999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999 days",null,1,[0,0,0,0,0,0,0]],["additional.nonfinite.237","9223372036854775808.5 years",null,1,[9223372036854775807,0,0,0,0,0,0]],["additional.nonfinite.238","9223372036854775808.5 months",null,1,[0,9223372036854775807,0,0,0,0,0]],["additional.nonfinite.239","9223372036854775808.5 days",null,1,[0,0,9223372036854775807,0,0,0,0]]]"#).unwrap();
        for row in cases.as_array().unwrap() {
            let id = row[0].as_str().unwrap();
            let input = if row[1] == "__MISSING__" {
                None
            } else {
                Some(&row[1])
            };
            let actual = InputInterval::parse(input, row[2].as_str());
            match row[3].as_u64().unwrap() {
                0 => assert_eq!(actual, Ok(None), "{id}"),
                1 => {
                    let expected = &row[4];
                    let values = expected
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_i64().unwrap())
                        .collect::<Vec<_>>();
                    assert_eq!(
                        actual,
                        Ok(Some(InputInterval {
                            years: values[0],
                            months: values[1],
                            days: values[2],
                            hours: values[3],
                            minutes: values[4],
                            seconds: values[5],
                            microseconds: values[6],
                            invert: false,
                            total_days: None
                        })),
                        "{id}"
                    );
                }
                _ => assert!(actual.is_err(), "{id}: {actual:?}"),
            }
        }
    }
}
