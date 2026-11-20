//! Passport intervals use the existing calendar implementation, including PHP carry.
use crate::OAuthError;
use openrust::support::{Date, DateInterval, DateTimezone, MonthOverflow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantLifetime {
    Seconds(u64),
    Interval(DateInterval),
    CalendarYears(u32),
}
impl GrantLifetime {
    /// Computes an expiry in UTC for explicit diagnostics and fixed vectors.
    pub fn expires_at(self, now: i64) -> Result<i64, OAuthError> {
        self.expires_at_in(now, DateTimezone::UTC)
    }
    /// Calendar years retain the supplied IANA timezone and carry leap days.
    pub fn expires_at_in(self, now: i64, timezone: DateTimezone) -> Result<i64, OAuthError> {
        let start = jiff::Timestamp::from_second(now).map_err(|_| invalid())?;
        let interval = match self {
            Self::Interval(interval) => interval,
            Self::Seconds(seconds) => DateInterval {
                microseconds: i64::try_from(seconds)
                    .ok()
                    .and_then(|v| v.checked_mul(1_000_000))
                    .ok_or_else(invalid)?,
                ..Default::default()
            },
            Self::CalendarYears(years) => DateInterval {
                years: i64::from(years),
                ..Default::default()
            },
        };
        Date::fromUtc(start, timezone)
            .plus(interval, MonthOverflow::Carry)
            .map(|d| {
                d.asDateTime()
                    .timestamp()
                    .as_nanosecond()
                    .div_euclid(1_000_000_000) as i64
            })
            .map_err(|_| invalid())
    }
    pub(crate) fn for_application(self, now: i64) -> Result<i64, OAuthError> {
        let current = Date::now(None).map_err(|_| invalid())?;
        self.expires_at_in(now, current.asDateTime().time_zone().clone())
    }
}
fn invalid() -> OAuthError {
    OAuthError::server_error("OAuth lifetime out of range")
}
