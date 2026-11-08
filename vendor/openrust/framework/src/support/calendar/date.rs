//! PHP/Carbon 请求日期解析的显式时钟、时区与扩展年份表示。

mod abbreviations;
mod civil;
mod format;
mod lexer;
#[cfg(test)]
mod numeric_timestamp_tests;
mod parse;
mod posix;
mod relative;
mod state;
mod tail_rules;
mod zone;

use crate::{Error, Result};
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::{
    fmt,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
pub use zone::ResolvedTimezone;

// calendar 的独立字符串工作预算；输入 bag 的 expanded-key 限额不覆盖字符串值。
const MAX_CALENDAR_BYTES: usize = 20 * 1024 * 1024;

/// 不受 chrono 年份范围限制的 Unix 秒和非负微秒。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalendarInstant {
    epoch_seconds: i64,
    microseconds: u32,
}
impl CalendarInstant {
    /// 从 Unix 秒和 `0..1_000_000` 微秒构造时刻。
    pub fn new(epoch_seconds: i64, microseconds: u32) -> Result<Self> {
        if microseconds >= 1_000_000 {
            return Err(range_error());
        }
        Ok(Self {
            epoch_seconds,
            microseconds,
        })
    }
    /// 返回向下取整的 Unix 秒。
    pub const fn epoch_seconds(self) -> i64 {
        self.epoch_seconds
    }
    /// 返回秒内的非负微秒。
    pub const fn microseconds(self) -> u32 {
        self.microseconds
    }
    /// 显式转换到 chrono；超出 chrono 范围时返回错误，不限制底层时刻。
    pub fn try_chrono(self) -> Result<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::from_timestamp(self.epoch_seconds, self.microseconds * 1000)
            .ok_or_else(range_error)
    }
}

/// 日期解析使用的时钟；请求和测试通过显式依赖提供。
pub trait CalendarClock: Send + Sync {
    /// 读取一次当前时刻；单次日期解析不会反复读取时钟。
    fn now(&self) -> Result<CalendarInstant>;
    /// 是否采用 Carbon test-now 的缺项/微秒/时区语义；生产时钟默认关闭。
    fn is_test_clock(&self) -> bool {
        false
    }
}
/// 从操作系统读取真实时间的时钟。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;
impl CalendarClock for SystemClock {
    fn now(&self) -> Result<CalendarInstant> {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => CalendarInstant::new(
                i64::try_from(duration.as_secs()).map_err(|_| range_error())?,
                duration.subsec_micros(),
            ),
            Err(error) => {
                let duration = error.duration();
                let micros = -(i128::from(duration.as_secs()) * 1_000_000
                    + i128::from(duration.subsec_micros()));
                CalendarInstant::new(
                    i64::try_from(micros.div_euclid(1_000_000)).map_err(|_| range_error())?,
                    u32::try_from(micros.rem_euclid(1_000_000)).map_err(|_| range_error())?,
                )
            }
        }
    }
}
/// 持有确定时刻并采用 Carbon test-now 语义的测试时钟；不修改进程全局状态。
#[derive(Clone, Copy, Debug)]
pub struct FixedClock(CalendarInstant);
impl FixedClock {
    /// 构造固定时钟。
    pub const fn new(instant: CalendarInstant) -> Self {
        Self(instant)
    }
}
impl CalendarClock for FixedClock {
    fn is_test_clock(&self) -> bool {
        true
    }
    fn now(&self) -> Result<CalendarInstant> {
        Ok(self.0)
    }
}

/// 当前应用或请求的默认时区与时钟，可通过请求 extensions 注入。
#[derive(Clone)]
pub struct CalendarContext {
    timezone: ResolvedTimezone,
    clock: Arc<dyn CalendarClock>,
    date_factory: Option<Arc<DateFactory>>,
}

type DateFactory = dyn for<'a> Fn(DateConstruction<'a>, &CalendarContext) -> Result<Option<InputDate>>
    + Send
    + Sync;

/// 由应用日期工厂处理的构造操作。
#[derive(Clone, Copy, Debug)]
pub enum DateConstruction<'a> {
    /// 从应用时钟取得当前日期。
    Now,
    /// 从已有日期取得值，保留时刻及原时区。
    Instance(&'a InputDate),
    /// 按应用时区从 Unix 时间戳构造，保留传入工厂的原始参数。
    FromTimestamp {
        /// 原始数值或数值字符串。
        source: &'a Value,
        /// 依 Carbon 数值规则解析的时刻。
        instant: CalendarInstant,
    },
    /// 按指定的 PHP 格式解析。
    FromFormat {
        /// 要解析的原值。
        value: &'a Value,
        /// PHP 日期格式。
        format: &'a str,
    },
    /// 按 Carbon 的宽松规则解析，空字符串取应用当前时刻。
    Parse(&'a Value),
}

impl DateConstruction<'_> {
    /// 执行默认构造，可供自定义工厂装饰。此入口直接使用 Calendar 解析内核。
    pub fn default_date(self, context: &CalendarContext) -> Result<Option<InputDate>> {
        match self {
            Self::Now => Ok(Some(InputDate::new(
                context.now()?,
                context.timezone().clone(),
            ))),
            Self::Instance(date) => Ok(Some(date.clone())),
            Self::FromTimestamp { instant, .. } => {
                Ok(Some(InputDate::new(instant, context.timezone().clone())))
            }
            Self::FromFormat { value, format } => parse_date(
                value,
                DateOptions {
                    format: Some(format),
                    timezone: None,
                },
                context,
            ),
            Self::Parse(Value::String(text)) if text.is_empty() => Ok(Some(InputDate::new(
                context.now()?,
                context.timezone().clone(),
            ))),
            Self::Parse(value) => parse_date(value, DateOptions::default(), context),
        }
    }
}
impl fmt::Debug for CalendarContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CalendarContext")
            .field("timezone", &self.timezone)
            .finish_non_exhaustive()
    }
}
impl Default for CalendarContext {
    fn default() -> Self {
        Self::new(ResolvedTimezone::utc(), Arc::new(SystemClock))
    }
}
impl CalendarContext {
    /// 使用明确的默认时区和时钟构造上下文。
    pub fn new(timezone: ResolvedTimezone, clock: Arc<dyn CalendarClock>) -> Self {
        Self {
            timezone,
            clock,
            date_factory: None,
        }
    }
    /// 为当前应用上下文替换日期构造。可调用 [`DateConstruction::default_date`] 后调整结果。
    #[must_use]
    pub fn with_date_factory(
        mut self,
        factory: impl for<'a> Fn(DateConstruction<'a>, &CalendarContext) -> Result<Option<InputDate>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.date_factory = Some(Arc::new(factory));
        self
    }
    /// 经应用工厂执行日期构造，未配置时使用默认规则。
    pub fn make_date(&self, operation: DateConstruction<'_>) -> Result<Option<InputDate>> {
        match &self.date_factory {
            Some(factory) => factory(operation, self),
            None => operation.default_date(self),
        }
    }
    /// 返回默认时区。
    pub const fn timezone(&self) -> &ResolvedTimezone {
        &self.timezone
    }
    /// 当前时钟是否显式启用 Carbon test-now 语义。
    pub fn is_test_clock(&self) -> bool {
        self.clock.is_test_clock()
    }
    /// 返回当前时钟的时刻。
    pub fn now(&self) -> Result<CalendarInstant> {
        self.clock.now()
    }
}

/// 显式覆盖日期解析所用时区的输入。
#[derive(Clone, Copy, Debug)]
pub enum TimezoneInput<'a> {
    /// IANA 名称、UTC 偏移或 PHP 时区缩写。
    Name(&'a str),
    /// Carbon 兼容的整数小时偏移。
    OffsetHours(i64),
}
impl<'a> From<&'a str> for TimezoneInput<'a> {
    fn from(value: &'a str) -> Self {
        Self::Name(value)
    }
}

/// PHP 格式及解析时区；格式字符串不是 chrono strftime 格式。
#[derive(Clone, Copy, Debug, Default)]
pub struct DateOptions<'a> {
    /// 缺省时采用 PHP/Carbon 的宽松解析。
    pub format: Option<&'a str>,
    /// 缺省时使用请求上下文时区；输入自带时区仍按 PHP 规则优先。
    pub timezone: Option<TimezoneInput<'a>>,
}

/// 带时区身份的日期，底层 Unix 时刻不受 chrono 的年份范围限制。
#[derive(Clone, Debug)]
pub struct InputDate {
    instant: CalendarInstant,
    timezone: ResolvedTimezone,
}
impl PartialEq for InputDate {
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant
    }
}
impl Eq for InputDate {}
impl InputDate {
    /// 将已解析的时刻关联到时区，不改变时刻。
    pub fn new(instant: CalendarInstant, timezone: ResolvedTimezone) -> Self {
        Self { instant, timezone }
    }
    /// 返回 Unix 时刻。
    pub const fn instant(&self) -> CalendarInstant {
        self.instant
    }
    /// 返回包含原始身份的时区。
    pub const fn timezone(&self) -> &ResolvedTimezone {
        &self.timezone
    }
    /// 返回 Unix 秒。
    pub const fn timestamp(&self) -> i64 {
        self.instant.epoch_seconds
    }
    /// 返回秒内微秒。
    pub const fn microseconds(&self) -> u32 {
        self.instant.microseconds
    }
    /// 返回本地公历年份，支持扩展年份。
    pub fn year(&self) -> Result<i64> {
        Ok(self.timezone.civil(self.instant)?.year)
    }
    /// 返回本地月份。
    pub fn month(&self) -> Result<i64> {
        Ok(self.timezone.civil(self.instant)?.month)
    }
    /// 返回本地日期。
    pub fn day(&self) -> Result<i64> {
        Ok(self.timezone.civil(self.instant)?.day)
    }
    /// 显式转换为固定偏移的 chrono 日期；不改变底层支持范围。
    pub fn try_chrono(&self) -> Result<chrono::DateTime<chrono::FixedOffset>> {
        let offset = chrono::FixedOffset::east_opt(self.timezone.offset_seconds(self.instant)?)
            .ok_or_else(range_error)?;
        Ok(self.instant.try_chrono()?.with_timezone(&offset))
    }
    /// 按 PHP 日期输出格式生成字符串。
    pub fn format(&self, pattern: &str) -> Result<String> {
        format::display(self, pattern)
    }
    /// 解析该日期在自身时区中的本地午夜。
    pub fn start_of_day(&self) -> Result<Self> {
        let midnight = self.timezone.civil(self.instant)?.midnight();
        Ok(Self::new(
            self.timezone.resolve(midnight)?,
            self.timezone.clone(),
        ))
    }

    /// 输出包含时区偏移的 ISO 8601 字符串。
    pub fn to_iso8601_string(&self) -> Result<String> {
        self.format("Y-m-d\\TH:i:sP")
    }
}
impl Serialize for InputDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let utc = Self::new(self.instant, ResolvedTimezone::utc());
        let output = utc.format("Y-m-d\\TH:i:s.u\\Z").map_err(S::Error::custom)?;
        serializer.serialize_str(&output)
    }
}

/// 将有限浮点时间戳按现有 PHP 六位 half-up Kernel 输出固定小数文本。
///
/// 输出始终含六位小数，负零归一为 `0.000000`；半数按 PHP half-up 规则舍入。
/// 不读取时钟或时区，也不解析为时刻。需要 `U.u` 字段语义时使用
/// [`parse_php_numeric_timestamp`]；这两个函数均不采用 Carbon timestamp 的转换流程。
/// 原生 PHP 可将非有限数格式化为 `nan` / `inf`，本函数的有限浮点输入域明确拒绝它们。
///
/// # Errors
/// NaN、正负无穷返回范围错误。
pub fn format_php_numeric_timestamp(value: f64) -> Result<String> {
    lexer::number_format_timestamp(value)
}

/// 按 PHP `NumericDate` 的数值、六位文本与 `U.u` 字段规则构造时刻。
///
/// 数字与数值字符串均先转换为 `f64`，再格式化为六位小数并由现有 `U.u` 扫描器解析。
/// 字符串首尾允许空格、TAB、LF、CR、VT、FF 六种 ASCII 空白，不裁剪 NUL 或 Unicode 空白。
/// 解析使用固定 UTC，不读取当前时钟或应用时区；空值不会转换为当前时刻。
///
/// `U.u` 按有符号秒与非负微秒分别解释：`-0.25` 得到秒 `0`、微秒 `250000`，
/// `-1.25` 得到秒 `-1`、微秒 `250000`，不同于数学负数微秒或 Carbon timestamp。
/// 大整数也经过浮点阶段，可能丢失精度；例如 `2^53 + 1` 按 `2^53` 解析。
///
/// # Errors
/// 非数字 JSON 类型、非法数值字符串、非有限数、超出 20 MiB 的字符串或 `U.u` 秒字段超限时失败。
/// 错误信息不包含原始输入。
pub fn parse_php_numeric_timestamp(value: &Value) -> Result<CalendarInstant> {
    let number = match value {
        Value::Number(number) => number.as_f64().ok_or_else(range_error)?,
        Value::String(text) => {
            if text.len() > MAX_CALENDAR_BYTES {
                return Err(limit_error());
            }
            let text = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
            if !lexer::is_numeric(text) {
                return Err(calendar_error("input_date_format_invalid"));
            }
            text.parse::<f64>().map_err(|_| range_error())?
        }
        _ => return Err(calendar_error("input_date_type_invalid")),
    };
    let normalized = format_php_numeric_timestamp(number)?;
    let parsed = format::parse(
        &normalized,
        "U.u",
        civil::Civil::epoch(),
        &ResolvedTimezone::utc(),
    )?;
    Ok(parsed.instant())
}

/// 从已规范化的输入值解析日期；空值返回 None，失败不回显输入内容。
pub fn parse_date(
    value: &Value,
    options: DateOptions<'_>,
    context: &CalendarContext,
) -> Result<Option<InputDate>> {
    let source_bytes = match value {
        Value::String(text) => text.len(),
        _ => 0,
    };
    let timezone_bytes = match options.timezone {
        Some(TimezoneInput::Name(name)) => name.len(),
        _ => 0,
    };
    let total = source_bytes
        .checked_add(options.format.map_or(0, str::len))
        .and_then(|size| size.checked_add(timezone_bytes))
        .ok_or_else(limit_error)?;
    if total > MAX_CALENDAR_BYTES {
        return Err(limit_error());
    }
    if matches!(value, Value::Null)
        || matches!(value, Value::String(text) if text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\0']).is_empty())
    {
        return Ok(None);
    }
    let timezone = match options.timezone {
        None => context.timezone.clone(),
        Some(TimezoneInput::Name(name)) => ResolvedTimezone::parse(name)?,
        Some(TimezoneInput::OffsetHours(hours)) => ResolvedTimezone::from_offset_hours(hours)?,
    };
    let now = context.now()?;
    let is_test = context.is_test_clock();
    let date = if let Some(pattern) = options.format {
        let text = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            Value::Bool(value) => {
                if *value {
                    "1".into()
                } else {
                    String::new()
                }
            }
            _ => return Err(calendar_error("input_date_type_invalid")),
        };
        let mut base = timezone.civil(now)?;
        if !is_test {
            base.microsecond = 0;
        }
        format::parse(&text, pattern, base, &timezone)?
    } else {
        parse::parse(value, now, &timezone, is_test)?
    };
    Ok(Some(date))
}

// Eloquent 的 is_numeric 分支包含短数字字符串，且采用应用时区。
pub(crate) fn model_numeric_date(value: &Value) -> Result<Option<CalendarInstant>> {
    let text = match value {
        Value::Number(number) if number.is_f64() => {
            lexer::number_format_timestamp(number.as_f64().ok_or_else(range_error)?)?
        }
        Value::Number(number) => number.to_string(),
        Value::String(text)
            if lexer::is_numeric(text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}'])) =>
        {
            if text.len() > MAX_CALENDAR_BYTES {
                return Err(limit_error());
            }
            return lexer::carbon_timestamp(text).map(Some);
        }
        _ => return Ok(None),
    };
    lexer::carbon_timestamp(&text).map(Some)
}

fn calendar_error(code: &'static str) -> Error {
    Error::custom_with_status(http::StatusCode::BAD_REQUEST, code, "日期输入无法解析")
}
fn range_error() -> Error {
    calendar_error("input_calendar_out_of_range")
}
fn limit_error() -> Error {
    Error::custom_with_status(
        http::StatusCode::PAYLOAD_TOO_LARGE,
        "input_limits_exceeded",
        "日期解析资源超过限制",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_context() -> Result<CalendarContext> {
        Ok(CalendarContext::new(
            ResolvedTimezone::utc(),
            Arc::new(FixedClock::new(CalendarInstant::new(
                1_577_896_225,
                123_456,
            )?)),
        ))
    }

    // 捕获 PHP grammar、format reset、Carbon 缺项与时区优先级的实际差分。
    #[test]
    fn matches_real_php_carbon_date_golden() -> Result<()> {
        let cases: Vec<Value> = serde_json::from_str::<Value>(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-golden.json"
        ))
        .and_then(|fixture| serde_json::from_value(fixture["cases"].clone()))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        let mut failures = Vec::new();
        for case in cases {
            let id = case["id"].as_str().unwrap_or("unknown");
            let input = if case["input"] == "__MISSING__" {
                &Value::Null
            } else {
                &case["input"]
            };
            let timezone = case["options"]["timezone"]
                .as_str()
                .map(TimezoneInput::Name)
                .or_else(|| {
                    case["options"]["timezone"]
                        .as_i64()
                        .map(TimezoneInput::OffsetHours)
                });
            let result = parse_date(
                input,
                DateOptions {
                    format: case["options"]["format"].as_str(),
                    timezone,
                },
                &context,
            );
            let expected = &case["outcome"];
            match (result, expected.get("ok")) {
                (Err(_), None) | (Ok(None), Some(Value::Null)) => {}
                (Ok(Some(date)), Some(want)) => {
                    let matches = date.format("Y-m-d H:i:s.u")?
                        == want["datetime"].as_str().unwrap_or("")
                        && date.timezone().name() == want["timezone"].as_str().unwrap_or("")
                        && i64::from(date.timezone().offset_seconds(date.instant())?)
                            == want["offset_seconds"].as_i64().unwrap_or(i64::MIN);
                    if !matches {
                        failures.push(id.to_owned());
                    }
                }
                _ => failures.push(id.to_owned()),
            }
        }
        assert!(failures.is_empty(), "日期差分失败: {failures:?}");
        Ok(())
    }

    // 防止把 chrono 的表示范围误当作底层 PHP 日期范围。
    #[test]
    fn extended_year_and_negative_epoch_keep_full_calendar_range() -> Result<()> {
        let context = fixed_context()?;
        let date = parse_date(
            &Value::String("+1000000-01-01".into()),
            DateOptions::default(),
            &context,
        )?;
        assert_eq!(
            date.as_ref().map(InputDate::timestamp),
            Some(31_494_784_780_800)
        );
        assert!(date.as_ref().is_some_and(|date| date.try_chrono().is_err()));
        let negative = InputDate::new(CalendarInstant::new(-1, 750_000)?, ResolvedTimezone::utc());
        assert_eq!(
            negative.format("Y-m-d H:i:s.u")?,
            "1969-12-31 23:59:59.750000"
        );
        Ok(())
    }

    // 防止将西半球回拨的较早时刻策略误用于欧洲和南半球。
    #[test]
    fn timezone_folds_and_gaps_follow_php_offset_inference() -> Result<()> {
        let context = fixed_context()?;
        for (zone, local, expected) in [
            ("America/New_York", "2024-11-03 01:30:00", "-04:00"),
            ("Europe/Berlin", "2024-10-27 02:30:00", "+01:00"),
            ("Europe/London", "2024-10-27 01:30:00", "+00:00"),
            ("Australia/Sydney", "2024-04-07 02:30:00", "+10:00"),
        ] {
            let parsed = parse_date(
                &Value::String(local.into()),
                DateOptions {
                    format: None,
                    timezone: Some(zone.into()),
                },
                &context,
            )?;
            assert_eq!(
                parsed.map(|date| date.format("P")).transpose()?,
                Some(expected.into())
            );
        }
        Ok(())
    }

    // 防止日期参数逃过工作预算或错误泄漏用户提供的格式、值与时区。
    #[test]
    fn calendar_work_budget_and_safe_errors_are_independent_of_bag_keys() -> Result<()> {
        let context = fixed_context()?;
        let invalid = parse_date(
            &Value::String("date-secret-sentinel".into()),
            DateOptions::default(),
            &context,
        )
        .err();
        assert!(invalid.is_some());
        assert!(invalid.is_none_or(|error| !error.to_string().contains("date-secret-sentinel")));
        let source = Value::String(" ".repeat(MAX_CALENDAR_BYTES));
        assert!(
            parse_date(
                &source,
                DateOptions {
                    format: Some("Y"),
                    timezone: None
                },
                &context
            )
            .is_err()
        );
        Ok(())
    }
    // 防止 2099 后固定尾偏移、无条件 400 年投影或 chrono 范围破坏命名时区。
    #[test]
    fn named_zones_preserve_php_future_rules_and_historical_lmt() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-zone-future-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        let rows = fixture["cases"]
            .as_array()
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
        for row in rows {
            let zone = row["zone"]
                .as_str()
                .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            let date = parse_date(
                &row["input"],
                DateOptions {
                    format: None,
                    timezone: Some(zone.into()),
                },
                &context,
            )?
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            assert_eq!(
                Some(date.timestamp()),
                row["php"]["epoch"].as_i64(),
                "{zone}: {}",
                row["input"]
            );
            assert_eq!(
                Some(i64::from(date.timezone().offset_seconds(date.instant())?)),
                row["php"]["offset"].as_i64(),
                "{zone}: {}",
                row["input"]
            );
            assert_eq!(date.format("T")?, row["php"]["abbr"].as_str().unwrap_or(""));
        }
        Ok(())
    }

    // 捕获字符串预算之内的深前缀堆栈耗尽，并固定 1023/1024 的独立操作边界。
    #[test]
    fn relative_prefixes_use_bounded_iterative_state() -> Result<()> {
        let context = fixed_context()?;
        let accepted = Value::String(format!("{}now", "first day of ".repeat(1023)));
        assert!(parse_date(&accepted, DateOptions::default(), &context)?.is_some());
        let rejected = Value::String(format!("{}now", "first day of ".repeat(1024)));
        let error = parse_date(&rejected, DateOptions::default(), &context).err();
        assert!(error.is_some_and(|error| error.code() == "input_limits_exceeded"));
        Ok(())
    }
    // 防止只覆盖常见 ISO 日期而遗漏固定 timelib production 的替代写法和优先级。
    #[test]
    fn matches_fixed_timelib_grammar_productions() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-grammar-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        let mut failures = Vec::new();
        for row in fixture["cases"]
            .as_array()
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?
        {
            let timezone = row["options"]["timezone"].as_str().map(TimezoneInput::Name);
            let result = parse_date(
                &row["input"],
                DateOptions {
                    format: row["options"]["format"].as_str(),
                    timezone,
                },
                &context,
            );
            let expected = &row["outcome"];
            let matches = match (result, expected.get("ok")) {
                (Err(_), None) | (Ok(None), Some(Value::Null)) => true,
                (Ok(Some(date)), Some(want)) => {
                    date.format("Y-m-d H:i:s.u")? == want["datetime"].as_str().unwrap_or("")
                        && date.timezone().name() == want["timezone"].as_str().unwrap_or("")
                }
                _ => false,
            };
            if !matches {
                failures.push(row["id"].clone());
            }
        }
        assert!(failures.is_empty(), "timelib grammar差分失败: {failures:?}");
        Ok(())
    }

    // 直接源规则必须覆盖分隔闭包、年份宽度、GNU年复用及Carbon timestamp前置转换。
    #[test]
    fn matches_source_lexical_boundaries_and_non_january_reference() -> Result<()> {
        for (source, context) in [
            (
                include_str!(
                    "../../../tests/http/fixtures/php85/calendar-date-lexical-golden.json"
                ),
                fixed_context()?,
            ),
            (
                include_str!(
                    "../../../tests/http/fixtures/php85/calendar-date-clock-lexical-golden.json"
                ),
                CalendarContext::new(
                    ResolvedTimezone::utc(),
                    Arc::new(FixedClock::new(CalendarInstant::new(
                        1_592_238_625,
                        123_456,
                    )?)),
                ),
            ),
        ] {
            let fixture: Value = serde_json::from_str(source)
                .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
            let mut failures = Vec::new();
            for row in fixture["cases"]
                .as_array()
                .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?
            {
                let actual = parse_date(&row["input"], DateOptions::default(), &context);
                match (actual, row["outcome"].get("ok")) {
                    (Ok(Some(date)), Some(expected)) => {
                        let rendered = date.format("Y-m-d H:i:s.u")?;
                        if Some(rendered.as_str()) != expected["datetime"].as_str()
                            || Some(date.timezone().name()) != expected["timezone"].as_str()
                        {
                            failures.push(format!(
                                "{} {}: {rendered} {} (expected {})",
                                row["id"],
                                row["input"],
                                date.timezone().name(),
                                expected["datetime"]
                            ));
                        }
                    }
                    (Err(_), None) => {}
                    (result, _) => {
                        failures.push(format!("{} {}: {result:?}", row["id"], row["input"]));
                    }
                }
            }
            assert!(failures.is_empty(), "source lexical差分失败: {failures:?}");
        }
        Ok(())
    }

    // 捕获 UTF-8 被 '?'整字符吞掉、后缀假接受、默认空白过收与分钟显示丢失实际秒偏移。
    #[test]
    fn matches_php_byte_guards_and_fractional_offset_output() -> Result<()> {
        let context = fixed_context()?;
        for source in [
            include_str!("../../../tests/http/fixtures/php85/calendar-date-byte-guard-golden.json"),
            include_str!(
                "../../../tests/http/fixtures/php85/calendar-date-format-guard-golden.json"
            ),
        ] {
            let fixture: Value = serde_json::from_str(source)
                .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
            let mut failures = Vec::new();
            for row in fixture["cases"]
                .as_array()
                .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?
            {
                let actual = parse_date(
                    &row["input"],
                    DateOptions {
                        format: row["options"]["format"].as_str(),
                        timezone: None,
                    },
                    &context,
                );
                match (actual, row["outcome"].get("ok")) {
                    (Ok(Some(date)), Some(expected)) => {
                        let rendered = date.format("Y-m-d H:i:s.u")?;
                        if Some(rendered.as_str()) != expected["datetime"].as_str()
                            || Some(date.timezone().name()) != expected["timezone"].as_str()
                        {
                            failures.push(format!(
                                "{}: {rendered} {} expected {}",
                                row["id"],
                                date.timezone().name(),
                                expected["datetime"]
                            ));
                        }
                    }
                    (Err(_), None) => {}
                    (result, _) => {
                        failures.push(format!("{}: {result:?}", row["id"]));
                    }
                }
            }
            assert!(failures.is_empty(), "PHP byte/guard差分失败: {failures:?}");
            if let Some(rows) = fixture["output"].as_array() {
                for row in rows {
                    let zone = row["zone"]
                        .as_str()
                        .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
                    let date = parse_date(
                        &row["input"],
                        DateOptions {
                            format: None,
                            timezone: Some(zone.into()),
                        },
                        &context,
                    )?
                    .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
                    let pattern = row["pattern"]
                        .as_str()
                        .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
                    assert_eq!(
                        Some(date.format(pattern)?.as_str()),
                        row["expected"].as_str(),
                        "{zone}"
                    );
                    assert_eq!(
                        Some(i64::from(date.timezone().offset_seconds(date.instant())?)),
                        row["offset"].as_i64(),
                        "{zone}"
                    );
                }
            }
        }
        Ok(())
    }

    // 验证真实数字类型、单一特殊状态覆盖及文本timezone prefix与显式options的不同合同。
    #[test]
    fn matches_carbon_numeric_special_and_timezone_productions() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-remaining-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        let mut failures = Vec::new();
        for row in fixture["cases"]
            .as_array()
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?
        {
            let input = &row["input"];
            let correct_type = match row["input_type"].as_str() {
                Some("int") => input.is_i64() || input.is_u64(),
                Some("float") => input.as_number().is_some_and(serde_json::Number::is_f64),
                Some("string") => input.is_string(),
                _ => false,
            };
            assert!(correct_type, "PHP input类型丢失: {}", row["id"]);
            let timezone = row["options"]["timezone"].as_str().map(TimezoneInput::Name);
            let actual = parse_date(
                input,
                DateOptions {
                    format: None,
                    timezone,
                },
                &context,
            );
            match (actual, row["outcome"].get("ok")) {
                (Ok(Some(date)), Some(expected)) => {
                    let rendered = date.format("Y-m-d H:i:s.u")?;
                    let displayed_zone = date.format("e")?;
                    let displayed_p = date.format("P")?;
                    let displayed_o = date.format("O")?;
                    if Some(rendered.as_str()) != expected["datetime"].as_str()
                        || Some(date.timezone().name()) != expected["timezone"].as_str()
                        || Some(displayed_zone.as_str()) != expected["format_e"].as_str()
                        || Some(displayed_p.as_str()) != expected["format_P"].as_str()
                        || Some(displayed_o.as_str()) != expected["format_O"].as_str()
                        || Some(i64::from(date.timezone().offset_seconds(date.instant())?))
                            != expected["offset_seconds"].as_i64()
                    {
                        failures.push(format!(
                            "{}: {rendered} name={} e={displayed_zone} expected={expected}",
                            row["id"],
                            date.timezone().name()
                        ));
                    }
                }
                (Err(error), None) => {
                    assert_eq!(
                        Some(u64::from(error.status().as_u16())),
                        row["rust_error"]["status"].as_u64(),
                        "{}",
                        row["id"]
                    );
                    assert_eq!(
                        Some(error.code()),
                        row["rust_error"]["code"].as_str(),
                        "{}",
                        row["id"]
                    );
                }
                (result, _) => {
                    failures.push(format!("{}: {result:?}", row["id"]));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "Carbon numeric/special/timezone差分失败: {failures:?}"
        );
        Ok(())
    }

    // 捕获 i128 MAX 加微秒进位的 panic，并断言与真实 PHP 同样拒绝正负及有无@的输入。
    #[test]
    fn rejects_timestamp_component_carry_overflow_without_panicking() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-overflow-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        for row in fixture["cases"]
            .as_array()
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?
        {
            assert!(
                row["outcome"].get("error").is_some(),
                "fixture必须保留真实PHP错误: {}",
                row["id"]
            );
            let error = parse_date(&row["input"], DateOptions::default(), &context).err();
            assert!(
                error.is_some_and(|error| error.status() == http::StatusCode::BAD_REQUEST
                    && error.code() == "input_date_invalid"),
                "{}",
                row["id"]
            );
        }
        Ok(())
    }

    // 默认 pgydotd 必须三位序数；显式 createFromFormat 的 z/month 溢出规则独立保留。
    #[test]
    fn implicit_ordinal_width_does_not_reinterpret_invalid_calendar_dates() -> Result<()> {
        let context = fixed_context()?;
        let month = parse_date(
            &Value::String("2020-2".into()),
            DateOptions::default(),
            &context,
        )?
        .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
        assert_eq!(
            month.format("Y-m-d H:i:s.u e")?,
            "2020-02-01 00:00:00.000000 UTC"
        );
        assert!(
            parse_date(
                &Value::String("2020-13-01".into()),
                DateOptions::default(),
                &context
            )
            .is_err()
        );
        let overflow = parse_date(
            &Value::String("2020-13-01".into()),
            DateOptions {
                format: Some("!Y-m-d"),
                timezone: None,
            },
            &context,
        )?
        .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
        assert_eq!(overflow.format("Y-m-d")?, "2021-01-01");
        Ok(())
    }

    // 捕获生产路径按输入位置即时计算、重复字段及 timezone 警告边界偏差。
    #[test]
    fn matches_fixed_timelib_composition_and_duplicate_fields() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-composition-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        let mut failures = Vec::new();
        for row in fixture["cases"]
            .as_array()
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?
        {
            let actual = parse_date(&row["input"], DateOptions::default(), &context);
            match (actual, row["outcome"].get("ok")) {
                (Ok(Some(date)), Some(expected)) => {
                    let rendered = date.format("Y-m-d H:i:s.u")?;
                    if Some(rendered.as_str()) != expected["datetime"].as_str()
                        || Some(date.timezone().name()) != expected["timezone"].as_str()
                    {
                        failures.push(format!(
                            "{}: {rendered} {}",
                            row["id"],
                            date.timezone().name()
                        ));
                    }
                }
                (Err(_), None) => {}
                (result, _) => failures.push(format!("{}: {result:?}", row["id"])),
            }
        }
        assert!(
            failures.is_empty(),
            "timelib composition差分失败: {failures:?}"
        );
        Ok(())
    }

    // 固定缩写 DST、数值 offset 的 GMT 输出及 UTC+1 Swatch beats 均可被公开格式调用观察。
    #[test]
    fn formats_beats_daylight_and_timezone_abbreviations() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-format-output-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        for zone in ["UTC", "+00:00", "+05:30", "Z", "EST", "EDT"] {
            let date = parse_date(
                &Value::String("2020-07-01 12:34:56".into()),
                DateOptions {
                    format: None,
                    timezone: Some(zone.into()),
                },
                &context,
            )?
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            for pattern in ["T", "I", "B"] {
                assert_eq!(
                    Some(date.format(pattern)?.as_str()),
                    fixture[zone][pattern].as_str(),
                    "{zone} {pattern}"
                );
            }
        }
        for (zone, input, expected) in [
            ("America/New_York", "3000-07-01 12:00:00", "EDT 1"),
            ("America/New_York", "3000-01-01 12:00:00", "EST 0"),
            ("Australia/Sydney", "3000-01-01 12:00:00", "AEDT 1"),
            ("Europe/Berlin", "1800-07-01 12:00:00", "LMT 0"),
        ] {
            let date = parse_date(
                &Value::String(input.into()),
                DateOptions {
                    format: None,
                    timezone: Some(zone.into()),
                },
                &context,
            )?
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            assert_eq!(date.format("T I")?, expected);
        }
        Ok(())
    }

    // 防止 signed 绝对值解析及周中日归一在 i64 epoch 边缘错误拒绝合法值。
    #[test]
    fn epoch_extremes_parse_and_iso_week_without_intermediate_instant_overflow() -> Result<()> {
        let fixture: Vec<Value> = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-extreme-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        let context = fixed_context()?;
        for row in fixture {
            let parsed = parse_date(
                &row["input"],
                DateOptions {
                    format: Some("!U"),
                    timezone: None,
                },
                &context,
            );
            if row.get("error").is_some() {
                assert!(parsed.is_err());
                continue;
            }
            let date = parsed?.ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            assert_eq!(date.format("U")?, row["ok"]["U"].as_str().unwrap_or(""));
            assert_eq!(
                date.format("Y-m-d H:i:s.u")?,
                row["ok"]["calendar"].as_str().unwrap_or("")
            );
            assert_eq!(
                date.format("Y-m-d N o-W")?,
                row["ok"]["iso_week"].as_str().unwrap_or("")
            );
        }
        Ok(())
    }

    // 防止生产时钟暗用 Carbon test-now 的微秒、无年日期与输入 zone 行为。
    #[test]
    fn production_clock_preserves_real_non_mock_carbon_branches() -> Result<()> {
        struct ObservedClock(CalendarInstant);
        impl CalendarClock for ObservedClock {
            fn now(&self) -> Result<CalendarInstant> {
                Ok(self.0)
            }
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/calendar-date-production-golden.json"
        ))
        .map_err(|_| calendar_error("calendar_test_fixture_invalid"))?;
        for row in fixture["cases"]
            .as_array()
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?
        {
            let epoch = row["observed_before"]["epoch"]
                .as_str()
                .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            let (seconds, micros) = epoch
                .split_once('.')
                .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            let instant = CalendarInstant::new(
                seconds.parse().map_err(|_| range_error())?,
                micros.parse().map_err(|_| range_error())?,
            )?;
            let context =
                CalendarContext::new(ResolvedTimezone::utc(), Arc::new(ObservedClock(instant)));
            assert!(!context.is_test_clock());
            let date = parse_date(
                &row["input"],
                DateOptions {
                    format: row["options"]["format"].as_str(),
                    timezone: None,
                },
                &context,
            )?
            .ok_or_else(|| calendar_error("calendar_test_fixture_invalid"))?;
            let expected = &row["outcome"]["ok"];
            let id = row["id"].as_str().unwrap_or("");
            if matches!(id, "production.now" | "production.relative") {
                // 真PHP调用位于已记录before/after窗口内；注入before作为确定clock，比较整秒并核保留该微秒。
                assert_eq!(
                    date.format("Y-m-d H:i:s")?,
                    expected["datetime"]
                        .as_str()
                        .unwrap_or("")
                        .get(..19)
                        .unwrap_or(""),
                    "{id}"
                );
                assert_eq!(date.microseconds(), instant.microseconds());
            } else {
                assert_eq!(
                    date.format("Y-m-d H:i:s.u")?,
                    expected["datetime"].as_str().unwrap_or(""),
                    "{id}"
                );
            }
            assert_eq!(
                date.timezone().name(),
                expected["timezone"].as_str().unwrap_or(""),
                "{id}"
            );
        }
        Ok(())
    }
}
