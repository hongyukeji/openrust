//! 休眠共用日期工厂时钟；冻结时只推进时间，真实休眠不阻塞异步执行器。
use super::{Date, DateError, DateFacade, DateFactory};
use crate::support::{
    DateInterval, MonthOverflow,
    calendar::{CalendarError, DateReference, InputInterval, parse_date_with_jiff},
    input_scalars,
};
use serde_json::Value;
impl DateFactory {
    pub async fn sleep(&self, seconds: f64) -> Result<(), DateError> {
        if !seconds.is_finite() {
            return Err(DateError::OutOfRange);
        }
        if self.clock.update_date(|date| advance(date, seconds))? || seconds <= 0.0 {
            return Ok(());
        }
        // 原生时钟将秒与小数秒分别截断到微秒；负值不等待。
        let duration = std::time::Duration::try_from_secs_f64(seconds.trunc())
            .map_err(|_| DateError::OutOfRange)?;
        let fraction = std::time::Duration::try_from_secs_f64(
            (seconds.fract() * 1_000_000.0).trunc() / 1_000_000.0,
        )
        .map_err(|_| DateError::OutOfRange)?;
        crate::support::Sleep::r#for(
            duration
                .checked_add(fraction)
                .ok_or(DateError::OutOfRange)?,
        )
        .wait()
        .await;
        Ok(())
    }
}
fn advance(date: &Date, seconds: f64) -> Result<Date, DateError> {
    if seconds == 0.0 {
        return Ok(date.clone());
    }
    let expression = format!("{} second", input_scalars::double_string(seconds.abs()));
    if let Ok(Some(interval)) = InputInterval::parse(Some(&Value::String(expression)), None) {
        let micros = interval
            .seconds
            .checked_mul(1_000_000)
            .and_then(|value| value.checked_add(interval.microseconds))
            .ok_or(DateError::OutOfRange)?;
        let micros = if seconds < 0.0 {
            micros.checked_neg().ok_or(DateError::OutOfRange)?
        } else {
            micros
        };
        return date.plus(
            DateInterval {
                microseconds: micros,
                ..Default::default()
            },
            MonthOverflow::Carry,
        );
    }
    // 科学计数文本可能由原区间构造器拒绝，沿用已有日期修改扫描器的后备路径。
    let value = parse_date_with_jiff(
        Value::String(format!("{} second", input_scalars::double_string(seconds))),
        date.as_date_time().time_zone().clone(),
        DateReference::Frozen(date.as_date_time().timestamp()),
    )
    .map_err(|error| match error {
        CalendarError::InvalidExpression => DateError::InvalidExpression,
        CalendarError::OutOfRange => DateError::OutOfRange,
        CalendarError::LimitExceeded => DateError::InputTooLarge,
    })?;
    Date::from_input(value)
}
impl DateFacade {
    pub async fn sleep(seconds: f64) -> Result<(), DateError> {
        Self::factory()?.sleep(seconds).await
    }
}
