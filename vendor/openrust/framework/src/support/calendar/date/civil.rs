//! 不受 chrono 年份范围限制的公历分量与归一化。

use super::{CalendarInstant, calendar_error, range_error};
use crate::Result;

#[derive(Clone, Copy, Debug)]
pub(super) struct Civil {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub microsecond: i64,
}

impl Civil {
    pub fn epoch() -> Self {
        Self {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
        }
    }
    pub fn from_instant(instant: CalendarInstant, offset: i32) -> Result<Self> {
        let seconds = i128::from(instant.epoch_seconds) + i128::from(offset);
        let days = seconds.div_euclid(86400);
        let time = seconds.rem_euclid(86400);
        let (year, month, day) = civil_from_days(days)?;
        Ok(Self {
            year,
            month,
            day,
            hour: i64::try_from(time / 3600).map_err(|_| range_error())?,
            minute: i64::try_from(time / 60 % 60).map_err(|_| range_error())?,
            second: i64::try_from(time % 60).map_err(|_| range_error())?,
            microsecond: i64::from(instant.microseconds),
        })
    }
    pub fn instant(self, offset: i32) -> Result<CalendarInstant> {
        let day = days_from_civil(self.year, self.month, self.day);
        let micros = day
            .checked_mul(86_400_000_000)
            .and_then(|v| v.checked_add(i128::from(self.hour) * 3_600_000_000))
            .and_then(|v| v.checked_add(i128::from(self.minute) * 60_000_000))
            .and_then(|v| v.checked_add(i128::from(self.second) * 1_000_000))
            .and_then(|v| v.checked_add(i128::from(self.microsecond)))
            .and_then(|v| v.checked_sub(i128::from(offset) * 1_000_000))
            .ok_or_else(range_error)?;
        CalendarInstant::new(
            i64::try_from(micros.div_euclid(1_000_000)).map_err(|_| range_error())?,
            u32::try_from(micros.rem_euclid(1_000_000)).map_err(|_| range_error())?,
        )
    }
    pub fn normalize(self) -> Result<Self> {
        Self::from_instant(self.instant(0)?, 0)
    }
    pub fn midnight(mut self) -> Self {
        self.hour = 0;
        self.minute = 0;
        self.second = 0;
        self.microsecond = 0;
        self
    }
    pub fn weekday(self) -> Result<i64> {
        i64::try_from((days_from_civil(self.year, self.month, self.day) + 4).rem_euclid(7))
            .map_err(|_| range_error())
    }
    pub fn add_days(&mut self, days: i64) -> Result<()> {
        self.day = self.day.checked_add(days).ok_or_else(range_error)?;
        *self = self.normalize()?;
        Ok(())
    }
    pub fn naive(self) -> Result<chrono::NaiveDateTime> {
        chrono::DateTime::from_timestamp(
            self.instant(0)?.epoch_seconds,
            u32::try_from(self.microsecond).map_err(|_| range_error())? * 1000,
        )
        .map(|date| date.naive_utc())
        .ok_or_else(|| calendar_error("input_calendar_timezone_range_unsupported"))
    }
}

pub(super) fn days_from_civil(year: i64, month: i64, day: i64) -> i128 {
    let month = i128::from(month) - 1;
    let mut year = i128::from(year) + month.div_euclid(12);
    let month = month.rem_euclid(12) + 1;
    year -= i128::from(month <= 2);
    let era = year.div_euclid(400);
    let year = year - era * 400;
    let month = month + if month > 2 { -3 } else { 9 };
    era * 146_097 + year * 365 + year / 4 - year / 100 + (153 * month + 2) / 5 + i128::from(day)
        - 1
        - 719_468
}

fn civil_from_days(days: i128) -> Result<(i64, i64, i64)> {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i128::from(month <= 2);
    Ok((
        i64::try_from(year).map_err(|_| range_error())?,
        i64::try_from(month).map_err(|_| range_error())?,
        i64::try_from(day).map_err(|_| range_error())?,
    ))
}

pub(super) fn days_in_month(year: i64, month: i64) -> Result<i64> {
    let start = days_from_civil(year, month, 1);
    let next = days_from_civil(year, month.checked_add(1).ok_or_else(range_error)?, 1);
    i64::try_from(next - start).map_err(|_| range_error())
}

// 只在 i128 公历日上选取周四，避免 i64 epoch 边缘往返时刻时越界。
pub(super) fn iso_week(year: i64, month: i64, day: i64) -> Result<(i64, i64)> {
    let days = days_from_civil(year, month, day);
    let thursday = days + 3 - (days + 3).rem_euclid(7);
    let (year, _, _) = civil_from_days(thursday)?;
    let first = days_from_civil(year, 1, 4);
    let monday = first - (first + 3).rem_euclid(7);
    Ok((
        year,
        i64::try_from((days - monday).div_euclid(7) + 1).map_err(|_| range_error())?,
    ))
}
