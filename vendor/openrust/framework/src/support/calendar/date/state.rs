//! timelib 聚合解析状态；按源顺序在扫描结束后统一应用相对分量。
//! 来源：PHP 8.5.0 `parse_date.re` / `tm2unixtime.c`；完整版权许可见 SOURCES.md / LICENSE.timelib。

use super::civil::Civil;
use super::format;
use super::lexer::{ProductionTag, Token};
use super::{InputDate, ResolvedTimezone, calendar_error, limit_error, range_error};
use crate::Result;

const MAX_OPERATIONS: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub(super) enum RelativeUnit {
    Year,
    Month,
    Week,
    Fortnight,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum FirstLast {
    First,
    Last,
}
#[derive(Clone, Copy, Debug)]
struct Weekday {
    day: i64,
    behavior: i64,
}
#[derive(Clone, Copy)]
enum Special {
    Weekdays(i64),
    OrdinalMonth { last: bool },
}
#[derive(Default)]
struct Relative {
    years: i64,
    months: i64,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    micros: i64,
    weekday: Option<Weekday>,
    first_last: Option<FirstLast>,
    special: Option<Special>,
}

/// 一次词法扫描的唯一累计状态；时钟在外部只读取一次。
// 这些正交标志对应 timelib have_date/have_time/warning 状态，不合并为互斥枚举。
#[allow(clippy::struct_excessive_bools)]
pub(super) struct ParseState {
    civil: Civil,
    timezone: ResolvedTimezone,
    reference: Option<super::CalendarInstant>,
    base_timezone: ResolvedTimezone,
    is_test: bool,
    date_mask: u16,
    have_date: bool,
    have_time: u8,
    time_touched: bool,
    zone_count: u8,
    operations: usize,
    relative: Relative,
}

impl ParseState {
    pub fn new(civil: Civil, timezone: ResolvedTimezone) -> Self {
        Self {
            civil,
            base_timezone: timezone.clone(),
            timezone,
            reference: None,
            is_test: false,
            date_mask: 0,
            have_date: false,
            have_time: 0,
            time_touched: false,
            zone_count: 0,
            operations: 0,
            relative: Relative::default(),
        }
    }
    pub fn reference(&mut self, instant: super::CalendarInstant, is_test: bool) {
        self.reference = Some(instant);
        self.is_test = is_test;
    }
    pub fn absolute(&mut self, token: &Token) -> Result<()> {
        self.operation()?;
        if token.tag == ProductionTag::Timestamp {
            return self.timestamp(token.civil);
        }
        if token.tag == ProductionTag::GnuTime && self.have_time != 0 {
            if self.have_time >= 2 {
                return Err(invalid());
            }
            self.civil.year = token.civil.hour * 100 + token.civil.minute;
            self.date_mask |= format::YEAR;
            self.have_time += 1;
            return Ok(());
        }
        let date_mask = token.mask & (format::YEAR | format::MONTH | format::DAY);
        if date_mask != 0 {
            if token.tag != ProductionTag::Year {
                if self.have_date {
                    return Err(invalid());
                }
                self.have_date = true;
            }
            self.date_mask |= date_mask;
            if date_mask & format::YEAR != 0 {
                self.civil.year = token.civil.year;
            }
            if date_mask & format::MONTH != 0 {
                self.civil.month = token.civil.month;
            }
            if date_mask & format::DAY != 0 {
                self.civil.day = token.civil.day;
            }
            if self.is_test && date_mask & format::YEAR == 0 {
                self.time_touched = true;
            }
        }
        if let Some(days) = token.relative_day {
            self.relative.days = days;
        }
        if token.mask & format::HOUR != 0 {
            let micros = if token.tag == ProductionTag::GnuTime
                && self.is_test
                && self.date_mask != (format::YEAR | format::MONTH | format::DAY)
            {
                self.civil.microsecond
            } else {
                token.civil.microsecond
            };
            self.set_time(
                token.civil.hour,
                token.civil.minute,
                token.civil.second,
                micros,
            )?;
        }
        Ok(())
    }
    fn timestamp(&mut self, value: Civil) -> Result<()> {
        self.have_date = false;
        self.date_mask = format::YEAR | format::MONTH | format::DAY;
        self.clear_time();
        self.civil.year = 0;
        self.civil.month = 0;
        self.civil.day = 0;
        let previous = self.zone_count;
        self.set_zone(ResolvedTimezone::parse("+00:00")?)?;
        // timelib 第二zone warning在重置date/time之后、timestamp贡献之前短路。
        if previous != 0 {
            return Ok(());
        }
        self.civil = Civil::epoch();
        let seconds = if value.microsecond < 0 {
            value.second.wrapping_sub(1)
        } else {
            value.second
        };
        self.relative.seconds = self
            .relative
            .seconds
            .checked_add(seconds)
            .ok_or_else(range_error)?;
        self.relative.micros = value.microsecond.rem_euclid(1_000_000);
        Ok(())
    }
    // 每个被接受的token/修饰调用恰计一次；该预算在分支、相对段和绝对段之间不重置。
    fn operation(&mut self) -> Result<()> {
        if self.operations >= MAX_OPERATIONS {
            return Err(limit_error());
        }
        self.operations += 1;
        Ok(())
    }
    #[cfg(test)]
    pub fn date(&mut self, civil: Civil, preserve_unspecified_time: bool) -> Result<()> {
        self.operation()?;
        if self.have_date {
            return Err(invalid());
        }
        self.have_date = true;
        self.civil.year = civil.year;
        self.civil.month = civil.month;
        self.civil.day = civil.day;
        if preserve_unspecified_time {
            self.time_touched = true;
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn time(&mut self, hour: i64, minute: i64, second: i64, micros: i64) -> Result<()> {
        self.operation()?;
        self.set_time(hour, minute, second, micros)
    }
    fn set_time(&mut self, hour: i64, minute: i64, second: i64, micros: i64) -> Result<()> {
        if self.have_time != 0 {
            return Err(invalid());
        }
        self.have_time = 1;
        self.time_touched = true;
        self.civil.hour = hour;
        self.civil.minute = minute;
        self.civil.second = second;
        self.civil.microsecond = micros;
        Ok(())
    }
    pub fn replace_time(&mut self, hour: i64, minute: i64) -> Result<()> {
        self.operation()?;
        self.clear_time();
        self.set_time(hour, minute, 0, 0)
    }
    fn clear_time(&mut self) {
        self.civil = self.civil.midnight();
        self.have_time = 0;
        self.time_touched = true;
    }
    pub fn reset_time(&mut self, day_delta: Option<i64>) -> Result<()> {
        self.operation()?;
        self.clear_time();
        if let Some(days) = day_delta {
            self.relative.days = days;
        }
        Ok(())
    }
    pub fn noon(&mut self) -> Result<()> {
        self.operation()?;
        self.clear_time();
        self.have_time = 1;
        self.civil.hour = 12;
        Ok(())
    }
    pub fn zone(&mut self, timezone: ResolvedTimezone) -> Result<()> {
        self.operation()?;
        self.set_zone(timezone)
    }
    fn set_zone(&mut self, timezone: ResolvedTimezone) -> Result<()> {
        if self.zone_count >= 2 {
            return Err(invalid());
        }
        if self.zone_count == 0 {
            self.timezone = timezone;
        }
        self.zone_count += 1;
        Ok(())
    }
    #[cfg(test)]
    pub fn has_duplicate_zone_warning(&self) -> bool {
        self.zone_count == 2
    }
    pub fn noop(&mut self) -> Result<()> {
        self.operation()
    }
    pub fn add(&mut self, unit: RelativeUnit, value: i64) -> Result<()> {
        self.operation()?;
        let (target, factor) = match unit {
            RelativeUnit::Year => (&mut self.relative.years, 1),
            RelativeUnit::Month => (&mut self.relative.months, 1),
            RelativeUnit::Week => (&mut self.relative.days, 7),
            RelativeUnit::Fortnight => (&mut self.relative.days, 14),
            RelativeUnit::Day => (&mut self.relative.days, 1),
            RelativeUnit::Hour => (&mut self.relative.hours, 1),
            RelativeUnit::Minute => (&mut self.relative.minutes, 1),
            RelativeUnit::Second => (&mut self.relative.seconds, 1),
            RelativeUnit::Millisecond => (&mut self.relative.micros, 1000),
            RelativeUnit::Microsecond => (&mut self.relative.micros, 1),
        };
        *target = target
            .checked_add(value.checked_mul(factor).ok_or_else(range_error)?)
            .ok_or_else(range_error)?;
        Ok(())
    }
    pub fn weekday(
        &mut self,
        day: i64,
        amount: Option<i64>,
        behavior: i64,
        keep_time: bool,
    ) -> Result<()> {
        self.operation()?;
        if !keep_time {
            self.clear_time();
        }
        if let Some(amount) = amount {
            let weeks = if amount > 0 { amount - 1 } else { amount };
            self.relative.days = self
                .relative
                .days
                .checked_add(weeks.checked_mul(7).ok_or_else(range_error)?)
                .ok_or_else(range_error)?;
        }
        let behavior = if amount.is_none()
            && self
                .relative
                .weekday
                .is_some_and(|value| value.behavior == 2)
        {
            2
        } else {
            behavior
        };
        self.relative.weekday = Some(Weekday { day, behavior });
        Ok(())
    }
    pub fn relative_week(&mut self, amount: i64) -> Result<()> {
        self.operation()?;
        self.relative.days = self
            .relative
            .days
            .checked_add(amount.checked_mul(7).ok_or_else(range_error)?)
            .ok_or_else(range_error)?;
        let day = self.relative.weekday.map_or(1, |weekday| weekday.day);
        self.relative.weekday = Some(Weekday { day, behavior: 2 });
        Ok(())
    }
    pub fn first_last(&mut self, which: FirstLast) -> Result<()> {
        self.operation()?;
        self.relative.first_last = Some(which);
        Ok(())
    }
    pub fn ordinal_weekday(&mut self, day: i64, amount: i64, behavior: i64) -> Result<()> {
        self.operation()?;
        self.clear_time();
        self.relative.special = Some(Special::OrdinalMonth { last: amount <= 0 });
        let weeks = if amount > 0 { amount - 1 } else { amount };
        self.relative.days = self
            .relative
            .days
            .checked_add(weeks.checked_mul(7).ok_or_else(range_error)?)
            .ok_or_else(range_error)?;
        self.relative.weekday = Some(Weekday {
            day,
            behavior: if amount > 0 { 1 } else { behavior },
        });
        Ok(())
    }
    pub fn weekdays(&mut self, amount: i64, keep_time: bool) -> Result<()> {
        self.operation()?;
        if !keep_time {
            self.clear_time();
        }
        self.relative.special = Some(Special::Weekdays(amount));
        Ok(())
    }
    pub fn ago(&mut self) -> Result<()> {
        self.operation()?;
        for value in [
            &mut self.relative.years,
            &mut self.relative.months,
            &mut self.relative.days,
            &mut self.relative.hours,
            &mut self.relative.minutes,
            &mut self.relative.seconds,
        ] {
            *value = value.checked_neg().ok_or_else(range_error)?;
        }
        // 固定 PHP ago 分支不翻转 relative.us；不以“修正”名义改掉源行为。
        if let Some(weekday) = &mut self.relative.weekday {
            weekday.day = weekday.day.checked_neg().ok_or_else(range_error)?;
            if weekday.day == 0 {
                weekday.day = -7;
            }
        }
        if let Some(Special::Weekdays(value)) = &mut self.relative.special {
            *value = value.checked_neg().ok_or_else(range_error)?;
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<InputDate> {
        // Carbon test-now 在日期依赖当前年份时保留 mock 时区；生产始终采用输入时区。
        if self.is_test && self.date_mask != (format::YEAR | format::MONTH | format::DAY) {
            self.timezone = self.base_timezone.clone();
        }
        if let Some(reference) = self.reference {
            let current = self.timezone.civil(reference)?;
            if self.date_mask & format::YEAR == 0 {
                self.civil.year = current.year;
            }
            if self.date_mask & format::MONTH == 0 {
                self.civil.month = current.month;
            }
            if self.date_mask & format::DAY == 0 {
                self.civil.day = current.day;
            }
            if !self.time_touched {
                self.civil.hour = current.hour;
                self.civil.minute = current.minute;
                self.civil.second = current.second;
                self.civil.microsecond = if !self.is_test && self.date_mask != 0 {
                    0
                } else {
                    current.microsecond
                };
            }
        }
        if self.have_date && !self.time_touched {
            self.civil = self.civil.midnight();
        }
        self.special_early()?;
        self.adjust_weekday()?;
        self.civil = self.civil.normalize()?;
        for (target, delta) in [
            (&mut self.civil.year, self.relative.years),
            (&mut self.civil.month, self.relative.months),
            (&mut self.civil.day, self.relative.days),
            (&mut self.civil.hour, self.relative.hours),
            (&mut self.civil.minute, self.relative.minutes),
            (&mut self.civil.second, self.relative.seconds),
            (&mut self.civil.microsecond, self.relative.micros),
        ] {
            *target = target.checked_add(delta).ok_or_else(range_error)?;
        }
        self.apply_first_last()?;
        self.civil = self.civil.normalize()?;
        if let Some(Special::Weekdays(amount)) = self.relative.special {
            special_weekdays(&mut self.civil, amount)?;
        }
        self.civil = self.civil.normalize()?;
        Ok(InputDate::new(
            self.timezone.resolve(self.civil)?,
            self.timezone,
        ))
    }
    fn apply_first_last(&mut self) -> Result<()> {
        match self.relative.first_last {
            Some(FirstLast::First) => self.civil.day = 1,
            Some(FirstLast::Last) => {
                self.civil.day = 0;
                self.civil.month = self.civil.month.checked_add(1).ok_or_else(range_error)?;
            }
            None => {}
        }
        Ok(())
    }
    fn special_early(&mut self) -> Result<()> {
        if let Some(Special::OrdinalMonth { last }) = self.relative.special {
            self.civil.day = 1;
            self.civil.month = self
                .civil
                .month
                .checked_add(self.relative.months)
                .and_then(|value| value.checked_add(i64::from(last)))
                .ok_or_else(range_error)?;
            self.relative.months = 0;
        }
        self.apply_first_last()?;
        self.civil = self.civil.normalize()?;
        Ok(())
    }
    fn adjust_weekday(&mut self) -> Result<()> {
        let Some(weekday) = self.relative.weekday else {
            return Ok(());
        };
        let current = self.civil.weekday()?;
        let mut target = weekday.day;
        let delta = if weekday.behavior == 2 {
            if current == 0 && target != 0 {
                target = target.checked_sub(7).ok_or_else(range_error)?;
            }
            if target == 0 && current != 0 {
                target = 7;
            }
            target.checked_sub(current).ok_or_else(range_error)?
        } else {
            let mut difference = target.checked_sub(current).ok_or_else(range_error)?;
            if (self.relative.days < 0 && difference < 0)
                || (self.relative.days >= 0 && difference <= -weekday.behavior)
            {
                difference = difference.checked_add(7).ok_or_else(range_error)?;
            }
            if target >= 0 {
                difference
            } else {
                target
                    .checked_abs()
                    .and_then(|value| value.checked_sub(current))
                    .and_then(|value| value.checked_sub(7))
                    .ok_or_else(range_error)?
            }
        };
        self.civil.day = self.civil.day.checked_add(delta).ok_or_else(range_error)?;
        Ok(())
    }
}

fn special_weekdays(civil: &mut Civil, count: i64) -> Result<()> {
    let dow = civil.weekday()?;
    let rem = count % 5;
    let adjustment = if count > 0 {
        if rem == 0 {
            if dow == 0 {
                -2
            } else if dow == 6 {
                -1
            } else {
                0
            }
        } else if dow == 6 {
            1
        } else if dow + rem > 5 {
            2
        } else {
            0
        }
    } else if rem == 0 {
        if dow == 6 { 2 } else { i64::from(dow == 0) }
    } else if dow == 0 {
        -1
    } else if dow + rem < 1 {
        -2
    } else {
        0
    };
    let weeks = (count / 5).checked_mul(7).ok_or_else(range_error)?;
    civil.day = civil
        .day
        .checked_add(weeks)
        .and_then(|value| value.checked_add(rem + adjustment))
        .ok_or_else(range_error)?;
    Ok(())
}
fn invalid() -> crate::Error {
    calendar_error("input_date_invalid")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> ParseState {
        ParseState::new(
            Civil {
                year: 2020,
                month: 1,
                day: 1,
                hour: 16,
                minute: 30,
                second: 25,
                microsecond: 123_456,
            },
            ResolvedTimezone::utc(),
        )
    }
    // 捕获按token即时改civil造成的“先加month后weekday”错误。
    #[test]
    fn weekday_is_adjusted_before_aggregate_months() -> Result<()> {
        let mut state = state();
        state.add(RelativeUnit::Month, 1)?;
        state.weekday(1, None, 1, false)?;
        assert_eq!(
            state.finish()?.format("Y-m-d H:i:s.u")?,
            "2020-02-06 00:00:00.000000"
        );
        Ok(())
    }
    // tomorrow清掉先前time，但后续time可重新提供；noon则会占有time。
    #[test]
    fn reset_and_explicit_time_preserve_scanner_order() -> Result<()> {
        let mut first = state();
        first.reset_time(Some(1))?;
        first.time(14, 30, 0, 0)?;
        assert_eq!(
            first.finish()?.format("Y-m-d H:i:s")?,
            "2020-01-02 14:30:00"
        );
        let mut last = state();
        last.time(14, 30, 0, 0)?;
        last.reset_time(Some(1))?;
        assert_eq!(last.finish()?.format("Y-m-d H:i:s")?, "2020-01-02 00:00:00");
        let mut noon = state();
        noon.noon()?;
        assert!(noon.time(14, 30, 0, 0).is_err());
        Ok(())
    }
    // 第二zone仅有界warning并保首zone，第三次才失败；不记录原始zone文本。
    #[test]
    fn duplicate_fields_follow_timelib_error_and_warning_boundaries() -> Result<()> {
        let mut zone = state();
        zone.zone(ResolvedTimezone::utc())?;
        zone.zone(ResolvedTimezone::parse("PST")?)?;
        assert!(zone.has_duplicate_zone_warning());
        assert!(zone.zone(ResolvedTimezone::parse("EST")?).is_err());
        let civil = Civil {
            year: 2020,
            month: 1,
            day: 2,
            ..Civil::epoch()
        };
        let mut dates = state();
        dates.date(civil, false)?;
        assert!(dates.date(civil, false).is_err());
        Ok(())
    }
    // 操作预算跨所有分支累计，不因绝对/相对段切换而重置。
    #[test]
    fn operation_budget_is_shared_across_token_categories() -> Result<()> {
        let mut state = state();
        for _ in 0..1023 {
            state.noop()?;
        }
        state.add(RelativeUnit::Day, 1)?;
        assert!(state.zone(ResolvedTimezone::utc()).is_err());
        Ok(())
    }
}
