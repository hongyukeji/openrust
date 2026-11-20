use jiff::{
    SignedDuration, Span, Timestamp, Zoned,
    civil::DateTime,
    tz::{AmbiguousOffset, Offset},
};

use super::facades::Config;

mod clock;
mod timezone;
pub(crate) use clock::{TestClock, frozen_utc_now, utc_now_for};

/// An IANA timezone used by OpenRust date helpers.
pub use jiff::tz::TimeZone as DateTimezone;

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Date(Zoned);

#[allow(non_snake_case, reason = "Matches Laravel's date helpers")]
impl Date {
    /// Resolve an IANA name using the framework's bundled IANA 2025b database.
    pub fn timezone(name: &str) -> Result<DateTimezone, DateError> {
        let (name, data) = timezone::get(name).ok_or(DateError::InvalidTimezone)?;
        DateTimezone::tzif(name, data).map_err(|_| DateError::InvalidTimezone)
    }

    pub fn fromUtc(value: Timestamp, timezone: DateTimezone) -> Self {
        Self(value.to_zoned(timezone))
    }

    pub fn fromLocal(value: DateTime, timezone: DateTimezone) -> Result<Self, DateError> {
        resolve(value, &timezone, None).map(Self)
    }

    pub fn now(timezone: Option<DateTimezone>) -> Result<Self, DateError> {
        now(timezone)
    }

    pub fn today(timezone: Option<DateTimezone>) -> Result<Self, DateError> {
        today(timezone)
    }

    pub fn asDateTime(&self) -> &Zoned {
        &self.0
    }

    pub fn startOfDay(&self) -> Result<Self, DateError> {
        Self::fromLocal(self.0.datetime().start_of_day(), self.0.time_zone().clone())
    }

    pub fn plus(&self, interval: DateInterval, overflow: MonthOverflow) -> Result<Self, DateError> {
        let years = interval
            .years
            .checked_mul(12)
            .ok_or(DateError::OutOfRange)?;
        let mut date = self.0.clone();
        for count in [years, interval.months] {
            if count != 0 {
                let local = shift_months(date.datetime(), count, overflow)?;
                date = resolve(local, date.time_zone(), Some(date.offset()))?;
            }
        }
        if interval.days != 0 {
            let days = Span::new()
                .try_days(interval.days)
                .map_err(|_| DateError::OutOfRange)?;
            let local = date
                .datetime()
                .checked_add(days)
                .map_err(|_| DateError::OutOfRange)?;
            date = resolve(local, date.time_zone(), Some(date.offset()))?;
        }
        date.checked_add(SignedDuration::from_micros(interval.microseconds))
            .map(Self)
            .map_err(|_| DateError::OutOfRange)
    }

    pub fn minus(
        &self,
        interval: DateInterval,
        overflow: MonthOverflow,
    ) -> Result<Self, DateError> {
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

pub fn now(timezone: Option<DateTimezone>) -> Result<Date, DateError> {
    let timezone = match timezone {
        Some(timezone) => timezone,
        None => match Config::get("app.timezone").map_err(|_| DateError::Configuration)? {
            Some(value) => Date::timezone(value.as_str().ok_or(DateError::InvalidTimezone)?)
                .map_err(|_| DateError::InvalidTimezone)?,
            None => DateTimezone::UTC,
        },
    };
    Ok(Date::fromUtc(clock::utc_now(), timezone))
}

pub fn today(timezone: Option<DateTimezone>) -> Result<Date, DateError> {
    now(timezone)?.startOfDay()
}

fn resolve(
    local: DateTime,
    timezone: &DateTimezone,
    preferred: Option<Offset>,
) -> Result<Zoned, DateError> {
    let ambiguous = timezone.to_ambiguous_zoned(local);
    let resolved = match ambiguous.offset() {
        AmbiguousOffset::Fold { after, .. } if Some(after) == preferred => ambiguous.later(),
        _ => ambiguous.compatible(),
    };
    resolved.map_err(|_| DateError::OutOfRange)
}

fn shift_months(
    local: DateTime,
    amount: i64,
    overflow: MonthOverflow,
) -> Result<DateTime, DateError> {
    let months = Span::new()
        .try_months(amount)
        .map_err(|_| DateError::OutOfRange)?;
    let shifted = local
        .checked_add(months)
        .map_err(|_| DateError::OutOfRange)?;
    if overflow == MonthOverflow::Carry {
        let excess = i64::from(local.day() - shifted.day());
        shifted
            .checked_add(Span::new().days(excess))
            .map_err(|_| DateError::OutOfRange)
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

/// Unix whole seconds, rounded down for instants before the epoch as PHP expects.
pub(crate) fn timestamp_seconds(timestamp: Timestamp) -> i64 {
    timestamp.as_nanosecond().div_euclid(1_000_000_000) as i64
}
