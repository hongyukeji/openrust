use chrono::{
    DateTime, Datelike, LocalResult, Months, NaiveDateTime, Offset, TimeDelta, TimeZone, Utc,
};
use chrono_tz::{GapInfo, Tz, TzOffset};

use super::facades::Config;

mod clock;
pub(crate) use clock::{TestClock, utc_now_for};

/// An IANA timezone used by OpenRust date helpers.
pub use chrono_tz::Tz as DateTimezone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateError {
    Configuration,
    InvalidTimezone,
    OutOfRange,
}

impl std::fmt::Display for DateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Configuration => "application date configuration is unavailable",
            Self::InvalidTimezone => "invalid application timezone",
            Self::OutOfRange => "date or interval is out of range",
        })
    }
}

impl std::error::Error for DateError {}

/// Signed civil components. Months and days retain their calendar meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DateInterval {
    pub years: i64,
    pub months: i64,
    pub days: i64,
    pub microseconds: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MonthOverflow {
    #[default]
    Carry,
    Clamp,
}

/// An immutable timestamp with its IANA timezone attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date(DateTime<Tz>);

#[allow(non_snake_case, reason = "Matches Laravel's date helpers")]
impl Date {
    pub fn fromUtc(value: DateTime<Utc>, timezone: Tz) -> Self {
        Self(value.with_timezone(&timezone))
    }

    pub fn fromLocal(value: NaiveDateTime, timezone: Tz) -> Result<Self, DateError> {
        resolve(value, timezone, None).map(Self)
    }

    pub fn now(timezone: Option<Tz>) -> Result<Self, DateError> {
        now(timezone)
    }

    pub fn today(timezone: Option<Tz>) -> Result<Self, DateError> {
        today(timezone)
    }

    pub fn asDateTime(&self) -> &DateTime<Tz> {
        &self.0
    }

    pub fn startOfDay(self) -> Result<Self, DateError> {
        let midnight = checked_local(&self.0)?
            .date()
            .and_hms_opt(0, 0, 0)
            .ok_or(DateError::OutOfRange)?;
        Self::fromLocal(midnight, self.0.timezone())
    }

    pub fn plus(self, interval: DateInterval, overflow: MonthOverflow) -> Result<Self, DateError> {
        let years = interval
            .years
            .checked_mul(12)
            .ok_or(DateError::OutOfRange)?;
        let mut date = self.0;
        for count in [years, interval.months] {
            if count != 0 {
                let local = shift_months(checked_local(&date)?, count, overflow)?;
                date = resolve(local, date.timezone(), Some(*date.offset()))?;
            }
        }
        if interval.days != 0 {
            let days = TimeDelta::try_days(interval.days).ok_or(DateError::OutOfRange)?;
            let local = checked_local(&date)?
                .checked_add_signed(days)
                .ok_or(DateError::OutOfRange)?;
            date = resolve(local, date.timezone(), Some(*date.offset()))?;
        }
        date.checked_add_signed(TimeDelta::microseconds(interval.microseconds))
            .map(Self)
            .ok_or(DateError::OutOfRange)
    }

    pub fn minus(self, interval: DateInterval, overflow: MonthOverflow) -> Result<Self, DateError> {
        self.plus(
            DateInterval {
                years: interval.years.checked_neg().ok_or(DateError::OutOfRange)?,
                months: interval.months.checked_neg().ok_or(DateError::OutOfRange)?,
                days: interval.days.checked_neg().ok_or(DateError::OutOfRange)?,
                microseconds: interval
                    .microseconds
                    .checked_neg()
                    .ok_or(DateError::OutOfRange)?,
            },
            overflow,
        )
    }
}

pub fn now(timezone: Option<Tz>) -> Result<Date, DateError> {
    let timezone = match timezone {
        Some(timezone) => timezone,
        None => match Config::get("app.timezone").map_err(|_| DateError::Configuration)? {
            Some(value) => value
                .as_str()
                .ok_or(DateError::InvalidTimezone)?
                .parse()
                .map_err(|_| DateError::InvalidTimezone)?,
            None => chrono_tz::UTC,
        },
    };
    Ok(Date::fromUtc(clock::utc_now(), timezone))
}

pub fn today(timezone: Option<Tz>) -> Result<Date, DateError> {
    now(timezone)?.startOfDay()
}

fn checked_local(date: &DateTime<Tz>) -> Result<NaiveDateTime, DateError> {
    date.naive_utc()
        .checked_add_signed(TimeDelta::seconds(i64::from(
            date.offset().fix().local_minus_utc(),
        )))
        .ok_or(DateError::OutOfRange)
}

fn resolve(
    local: NaiveDateTime,
    timezone: Tz,
    preferred: Option<TzOffset>,
) -> Result<DateTime<Tz>, DateError> {
    match timezone.from_local_datetime(&local) {
        LocalResult::Single(date) => Ok(date),
        LocalResult::Ambiguous(earlier, later) => Ok(if Some(*later.offset()) == preferred {
            later
        } else {
            earlier
        }),
        LocalResult::None => {
            let gap = GapInfo::new(&local, &timezone).ok_or(DateError::OutOfRange)?;
            let (start, _) = gap.begin.ok_or(DateError::OutOfRange)?;
            gap.end
                .and_then(|end| end.checked_add_signed(local.signed_duration_since(start)))
                .ok_or(DateError::OutOfRange)
        }
    }
}

fn shift_months(
    local: NaiveDateTime,
    amount: i64,
    overflow: MonthOverflow,
) -> Result<NaiveDateTime, DateError> {
    let months = u32::try_from(amount.unsigned_abs())
        .map(Months::new)
        .map_err(|_| DateError::OutOfRange)?;
    let shifted = if amount < 0 {
        local.checked_sub_months(months)
    } else {
        local.checked_add_months(months)
    }
    .ok_or(DateError::OutOfRange)?;
    if overflow == MonthOverflow::Carry {
        let excess = i64::from(local.day() - shifted.day());
        shifted
            .checked_add_signed(TimeDelta::days(excess))
            .ok_or(DateError::OutOfRange)
    } else {
        Ok(shifted)
    }
}

pub fn years(value: i64) -> DateInterval {
    DateInterval {
        years: value,
        ..Default::default()
    }
}

pub fn months(value: i64) -> DateInterval {
    DateInterval {
        months: value,
        ..Default::default()
    }
}

pub fn weeks(value: i64) -> Result<DateInterval, DateError> {
    Ok(DateInterval {
        days: value.checked_mul(7).ok_or(DateError::OutOfRange)?,
        ..Default::default()
    })
}

pub fn days(value: f64) -> Result<DateInterval, DateError> {
    let whole = checked_integer(value.trunc())?;
    let remainder = fixed_interval(value.fract(), MICROS_PER_DAY)?;
    Ok(DateInterval {
        days: whole,
        ..remainder
    })
}

pub fn hours(value: f64) -> Result<DateInterval, DateError> {
    fixed_interval(value, 3_600_000_000.0)
}

pub fn minutes(value: f64) -> Result<DateInterval, DateError> {
    fixed_interval(value, 60_000_000.0)
}

pub fn seconds(value: f64) -> Result<DateInterval, DateError> {
    fixed_interval(value, 1_000_000.0)
}

pub fn milliseconds(value: f64) -> Result<DateInterval, DateError> {
    fixed_interval(value, 1_000.0)
}

pub fn microseconds(value: f64) -> Result<DateInterval, DateError> {
    fixed_interval(value, 1.0)
}

const MICROS_PER_DAY: f64 = 86_400_000_000.0;

fn fixed_interval(value: f64, scale: f64) -> Result<DateInterval, DateError> {
    Ok(DateInterval {
        microseconds: checked_integer((value * scale).trunc())?,
        ..Default::default()
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Input is checked to be an in-range integer"
)]
fn checked_integer(value: f64) -> Result<i64, DateError> {
    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
        Ok(value as i64)
    } else {
        Err(DateError::OutOfRange)
    }
}
