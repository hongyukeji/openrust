// 日期/延迟辅助契约；许可见 ../console/view/components/LICENSE。
use super::{Date, DateError, DateInterval, facades::Date as DateFacade, timestamp_seconds};
use std::time::{SystemTime, UNIX_EPOCH};
/// 延迟秒数、绝对日期或日历区间；区间保留月份和本地日期含义。
#[derive(Clone, Debug, PartialEq)]
pub enum TimeDelay {
    Seconds(i64),
    Date(Date),
    Interval(DateInterval),
}
impl From<i64> for TimeDelay {
    fn from(value: i64) -> Self {
        Self::Seconds(value)
    }
}
impl From<Date> for TimeDelay {
    fn from(value: Date) -> Self {
        Self::Date(value)
    }
}
impl From<&Date> for TimeDelay {
    fn from(value: &Date) -> Self {
        Self::Date(value.clone())
    }
}
impl From<DateInterval> for TimeDelay {
    fn from(value: DateInterval) -> Self {
        Self::Interval(value)
    }
}
/// 可组合的延迟和耗时行为；关联错误保留使用方已有的错误类型。
pub trait InteractsWithTime {
    type Error: From<DateError>;
    fn seconds_until(&self, delay: impl Into<TimeDelay>) -> Result<i64, Self::Error> {
        match self.parse_date_interval(delay)? {
            TimeDelay::Seconds(value) => Ok(value),
            TimeDelay::Date(value) => timestamp_seconds(value.as_date_time().timestamp())
                .checked_sub(self.current_time()?)
                .map(|n| n.max(0))
                .ok_or_else(|| DateError::OutOfRange.into()),
            TimeDelay::Interval(_) => Err(DateError::InvalidExpression.into()),
        }
    }
    fn available_at(&self, delay: impl Into<TimeDelay>) -> Result<i64, Self::Error> {
        match self.parse_date_interval(delay)? {
            TimeDelay::Date(value) => Ok(timestamp_seconds(value.as_date_time().timestamp())),
            TimeDelay::Seconds(seconds) => {
                if seconds.checked_abs().is_none() {
                    // 绝对值无法保留整数时，源日期单位逻辑回退到日期修改表达式。
                    let input = super::calendar::modify_date_input(
                        &format!("{seconds} second"),
                        &DateFacade::now(None)?,
                    )
                    .map_err(|_| DateError::OutOfRange)?;
                    let value = Date::from_input(input)?;
                    return Ok(timestamp_seconds(value.as_date_time().timestamp()));
                }
                let now = DateFacade::now(None)?;
                // 只返回时间戳，无须构造超出原生日期范围的中间日期；保留源整秒加法的回绕。
                Ok(timestamp_seconds(now.as_date_time().timestamp()).wrapping_add(seconds))
            }
            TimeDelay::Interval(_) => Err(DateError::InvalidExpression.into()),
        }
    }
    fn parse_date_interval(&self, delay: impl Into<TimeDelay>) -> Result<TimeDelay, Self::Error> {
        match delay.into() {
            TimeDelay::Interval(interval) => {
                let input =
                    super::calendar::add_date_interval_input(&DateFacade::now(None)?, interval)
                        .map_err(|error| match error {
                            super::calendar::CalendarError::LimitExceeded => {
                                DateError::InputTooLarge
                            }
                            super::calendar::CalendarError::InvalidExpression => {
                                DateError::InvalidExpression
                            }
                            super::calendar::CalendarError::OutOfRange => DateError::OutOfRange,
                        })?;
                let date = Date::from_input(input)?;
                Ok(TimeDelay::Date(date))
            }
            value => Ok(value),
        }
    }
    fn current_time(&self) -> Result<i64, Self::Error> {
        Ok(timestamp_seconds(
            DateFacade::now(None)?.as_date_time().timestamp(),
        ))
    }
    fn run_time_for_humans(&mut self, start: f64, end: Option<f64>) -> Result<String, Self::Error> {
        run_time_for_humans(start, end).map_err(|_| DateError::OutOfRange.into())
    }
}
pub(crate) enum RunTimeError {
    Format,
    Integer(std::num::ParseIntError),
}
pub(crate) fn run_time_for_humans(start: f64, end: Option<f64>) -> Result<String, RunTimeError> {
    let end = match end {
        Some(end) => end,
        None => wall_time(),
    };
    let milliseconds = (end - start) * 1000.0;
    if milliseconds.is_nan() {
        return Ok("nanms".into());
    }
    if milliseconds.is_infinite() {
        return Ok(if milliseconds.is_sign_positive() {
            "0s"
        } else {
            "infms"
        }
        .into());
    }
    if milliseconds <= 1000.0 {
        let formatted = crate::support::calendar::format_decimal_number(milliseconds, 2)
            .map_err(|_| RunTimeError::Format)?;
        let (integer, fraction) = formatted.split_once('.').expect("固定精度包含小数点");
        let (sign, digits) = integer
            .strip_prefix('-')
            .map_or(("", integer), |n| ("-", n));
        let mut result = String::from(sign);
        for (i, digit) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                result.push(',');
            }
            result.push(digit);
        }
        return Ok(format!("{result}.{fraction}ms"));
    }
    // 短单位显示向下取整秒；月份按四周、年份按十二个月进位。
    let mut seconds = format!("{:.0}", (milliseconds / 1000.0).floor())
        .parse::<i64>()
        .map_err(RunTimeError::Integer)?;
    let mut parts = Vec::new();
    for (unit, divisor, plural) in [
        ("yr", 29_030_400, "yrs"),
        ("mo", 2_419_200, "mos"),
        ("w", 604_800, "w"),
        ("d", 86_400, "d"),
        ("h", 3600, "h"),
        ("m", 60, "m"),
        ("s", 1, "s"),
    ] {
        let amount = seconds / divisor;
        seconds %= divisor;
        if amount > 0 {
            parts.push(format!(
                "{amount}{}",
                if amount == 1 { unit } else { plural }
            ));
        }
    }
    Ok(parts.join(" "))
}
pub(crate) fn wall_time() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs_f64(),
        Err(error) => -error.duration().as_secs_f64(),
    }
}
