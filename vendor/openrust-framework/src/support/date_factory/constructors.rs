//! 分量创建沿用既有格式解析器；显式 None 使用当前时钟，Default 保留创建器的固定默认值。
use super::{Date, DateError, DateFacade, DateFactory, DateTimezone};
use crate::support::{DateInterval, MonthOverflow};

#[derive(Debug, Clone, Copy)]
pub struct DateComponents {
    pub year: Option<i64>,
    pub month: Option<i64>,
    pub day: Option<i64>,
    pub hour: Option<i64>,
    pub minute: Option<i64>,
    pub second: Option<f64>,
}
impl Default for DateComponents {
    fn default() -> Self {
        Self {
            year: Some(0),
            month: Some(1),
            day: Some(1),
            hour: Some(0),
            minute: Some(0),
            second: Some(0.0),
        }
    }
}
impl DateComponents {
    pub const fn current() -> Self {
        Self {
            year: None,
            month: None,
            day: None,
            hour: None,
            minute: None,
            second: None,
        }
    }
}
/// 严格创建的整数分量；类型系统拒绝小数参数，不执行 PHP 弱类型隐式截断。
#[derive(Debug, Clone, Copy)]
pub struct StrictDateComponents {
    pub year: Option<i64>,
    pub month: Option<i64>,
    pub day: Option<i64>,
    pub hour: Option<i64>,
    pub minute: Option<i64>,
    pub second: Option<i64>,
}
impl Default for StrictDateComponents {
    fn default() -> Self {
        Self {
            year: Some(0),
            month: Some(1),
            day: Some(1),
            hour: Some(0),
            minute: Some(0),
            second: Some(0),
        }
    }
}
impl StrictDateComponents {
    pub const fn current() -> Self {
        Self {
            year: None,
            month: None,
            day: None,
            hour: None,
            minute: None,
            second: None,
        }
    }
}
impl From<StrictDateComponents> for DateComponents {
    #[allow(
        clippy::cast_precision_loss,
        reason = "秒分量在创建时检查0到99，超出范围明确拒绝；有效范围可精确转换"
    )]
    fn from(parts: StrictDateComponents) -> Self {
        Self {
            year: parts.year,
            month: parts.month,
            day: parts.day,
            hour: parts.hour,
            minute: parts.minute,
            second: parts.second.map(|second| second as f64),
        }
    }
}
impl DateFactory {
    pub fn create(
        &self,
        parts: DateComponents,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        self.create_components(parts, timezone, self.is_strict_mode_enabled())
    }
    fn create_components(
        &self,
        parts: DateComponents,
        timezone: Option<DateTimezone>,
        strict: bool,
    ) -> Result<Option<Date>, DateError> {
        let now = if let Some(now) = self.get_test_now() {
            timezone.as_ref().map_or_else(
                || now.clone(),
                |zone| Date::from_utc(now.as_date_time().timestamp(), zone.clone()),
            )
        } else {
            self.now(timezone.clone())?
        };
        let value = now.as_date_time();
        let year = parts.year.unwrap_or(i64::from(value.year()));
        if !(-9999..=9999).contains(&year) {
            return Err(DateError::OutOfRange);
        }
        let month = parts.month.unwrap_or(i64::from(value.month()));
        let day = parts.day.unwrap_or(i64::from(value.day()));
        let hour = parts.hour.unwrap_or(i64::from(value.hour()));
        let minute = parts.minute.unwrap_or(i64::from(value.minute()));
        let second = parts.second.unwrap_or(
            f64::from(value.second()) + f64::from(value.subsec_nanosecond() / 1000) / 1_000_000.0,
        );
        if !second.is_finite() {
            return Err(DateError::OutOfRange);
        }
        if strict
            && (!(0..=99).contains(&month)
                || !(0..=99).contains(&day)
                || !(0..=99).contains(&hour)
                || !(0..=99).contains(&minute)
                || !(0.0..=99.0).contains(&second))
        {
            return Err(DateError::OutOfRange);
        }
        let seconds = crate::support::calendar::date_component_seconds(second)
            .map_err(|_| DateError::OutOfRange)?;
        let seconds = if second < 10.0 {
            format!("0{seconds}")
        } else {
            seconds
        };
        // 负年份与原创建器相同：先建立0年，再做日历年运算。
        let base_year = year.max(0);
        let text = format!("{base_year}-{month}-{day} {hour}:{minute:02}:{seconds}");
        let created = self.raw_create_from_format("!Y-n-j G:i:s.u", text, timezone)?;
        let created = created
            .map(|date| {
                if year < 0 {
                    date.plus(
                        DateInterval {
                            years: year,
                            ..Default::default()
                        },
                        MonthOverflow::Carry,
                    )
                } else {
                    Ok(date)
                }
            })
            .transpose()?;
        if strict && created.is_none() {
            return Err(DateError::InvalidExpression);
        }
        Ok(created)
    }
    pub fn create_strict(
        &self,
        parts: StrictDateComponents,
        timezone: Option<DateTimezone>,
    ) -> Result<Date, DateError> {
        self.create_components(parts.into(), timezone, true)?
            .ok_or(DateError::InvalidExpression)
    }
    pub fn create_safe(
        &self,
        parts: DateComponents,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        let fields = [
            (parts.year, 1, 9999),
            (parts.month, 1, 12),
            (parts.day, 1, 31),
            (parts.hour, 0, 23),
            (parts.minute, 0, 59),
        ];
        let invalid = fields
            .iter()
            .any(|(field, min, max)| field.is_some_and(|field| field < *min || field > *max))
            || parts.second.is_some_and(|second| {
                !second.is_finite() || second.fract() != 0.0 || !(0.0..=59.0).contains(&second)
            });
        if invalid {
            return self.invalid_safe_date();
        }
        let Some(created) = self.create(parts, timezone)? else {
            return self.invalid_safe_date();
        };
        let value = created.as_date_time();
        let actual = [
            i64::from(value.year()),
            i64::from(value.month()),
            i64::from(value.day()),
            i64::from(value.hour()),
            i64::from(value.minute()),
        ];
        if fields
            .iter()
            .zip(actual)
            .any(|((field, _, _), actual)| field.is_some_and(|field| field != actual))
            || parts
                .second
                .is_some_and(|second| second != f64::from(value.second()))
        {
            return self.invalid_safe_date();
        }
        Ok(Some(created))
    }
    fn invalid_safe_date(&self) -> Result<Option<Date>, DateError> {
        if self.is_strict_mode_enabled() {
            Err(DateError::InvalidExpression)
        } else {
            Ok(None)
        }
    }
    pub fn create_from_date(
        &self,
        year: Option<i64>,
        month: Option<i64>,
        day: Option<i64>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        self.create(
            DateComponents {
                year,
                month,
                day,
                ..DateComponents::current()
            },
            timezone,
        )
    }
    pub fn create_midnight_date(
        &self,
        year: Option<i64>,
        month: Option<i64>,
        day: Option<i64>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        self.create(
            DateComponents {
                year,
                month,
                day,
                ..Default::default()
            },
            timezone,
        )
    }
    pub fn create_from_time_string(
        &self,
        time: &str,
        timezone: Option<DateTimezone>,
    ) -> Result<Date, DateError> {
        use crate::support::calendar::{CalendarError, modify_date_input};
        let today = self.today(timezone)?;
        let time = if time.contains(':') {
            std::borrow::Cow::Borrowed(time)
        } else {
            std::borrow::Cow::Owned(format!("{time}:0"))
        };
        let result = modify_date_input(&time, &today).map_err(|error| match error {
            CalendarError::InvalidExpression => DateError::InvalidExpression,
            CalendarError::OutOfRange => DateError::OutOfRange,
            CalendarError::LimitExceeded => DateError::InputTooLarge,
        })?;
        Date::from_input(result)
    }
    pub fn create_from_time(
        &self,
        hour: Option<i64>,
        minute: Option<i64>,
        second: Option<f64>,
        timezone: Option<DateTimezone>,
    ) -> Result<Date, DateError> {
        self.create(
            DateComponents {
                hour,
                minute,
                second,
                ..DateComponents::current()
            },
            timezone,
        )?
        .ok_or(DateError::InvalidExpression)
    }
}
impl DateFacade {
    pub fn create(
        parts: DateComponents,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        Self::factory()?.create(parts, timezone)
    }
    pub fn create_safe(
        parts: DateComponents,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        Self::factory()?.create_safe(parts, timezone)
    }
    pub fn create_strict(
        parts: StrictDateComponents,
        timezone: Option<DateTimezone>,
    ) -> Result<Date, DateError> {
        Self::factory()?.create_strict(parts, timezone)
    }
    pub fn create_from_date(
        year: Option<i64>,
        month: Option<i64>,
        day: Option<i64>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        Self::factory()?.create_from_date(year, month, day, timezone)
    }
    pub fn create_midnight_date(
        year: Option<i64>,
        month: Option<i64>,
        day: Option<i64>,
        timezone: Option<DateTimezone>,
    ) -> Result<Option<Date>, DateError> {
        Self::factory()?.create_midnight_date(year, month, day, timezone)
    }
    pub fn create_from_time_string(
        time: &str,
        timezone: Option<DateTimezone>,
    ) -> Result<Date, DateError> {
        Self::factory()?.create_from_time_string(time, timezone)
    }
    pub fn create_from_time(
        hour: Option<i64>,
        minute: Option<i64>,
        second: Option<f64>,
        timezone: Option<DateTimezone>,
    ) -> Result<Date, DateError> {
        Self::factory()?.create_from_time(hour, minute, second, timezone)
    }
}
