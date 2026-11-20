use std::error::Error;

use jiff::Timestamp;

use crate::support::{Date, DateError, DateInterval, MonthOverflow, facades::application_scope};

use super::TestApp;

type TimeResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[allow(non_snake_case, reason = "Matches Laravel's testing time controls")]
impl TestApp {
    /// Freezes the fixture's clock at the given instant.
    pub fn travelTo(&self, time: Date) {
        self.clock.set(Some(time.asDateTime().timestamp()));
    }

    /// Freezes the fixture's clock at its current time and returns that time.
    pub async fn freezeTime(&mut self) -> TimeResult<Date> {
        let time = self.current_time().await?;
        self.travelTo(time.clone());
        Ok(time)
    }

    /// Freezes the fixture's clock at the beginning of its current second.
    pub async fn freezeSecond(&mut self) -> TimeResult<Date> {
        let time = self.current_second().await?;
        self.travelTo(time.clone());
        Ok(time)
    }

    /// Restores the system clock and returns the fixture's current time.
    pub async fn travelBack(&mut self) -> TimeResult<Date> {
        self.clock.set(None);
        self.current_time().await
    }

    /// Begins a relative jump by a signed number of units.
    pub fn travel(&mut self, amount: i64) -> Wormhole<'_> {
        Wormhole { app: self, amount }
    }

    /// Runs an async callback at the given time in this application's scope.
    pub async fn travelToFor<T>(
        &mut self,
        time: Date,
        callback: impl AsyncFnOnce(&mut TestApp, Date) -> TimeResult<T>,
    ) -> TimeResult<T> {
        let clock = self.clock.clone();
        let container = self.app().container_handle();
        clock
            .scope(time.asDateTime().timestamp(), async {
                self.boot().await?;
                application_scope(container, callback(self, time)).await
            })
            .await
    }

    /// Runs an async callback with the current time frozen for its duration.
    pub async fn freezeTimeFor<T>(
        &mut self,
        callback: impl AsyncFnOnce(&mut TestApp, Date) -> TimeResult<T>,
    ) -> TimeResult<T> {
        let now = self.current_time().await?;
        self.travelToFor(now, callback).await
    }

    /// Runs an async callback with the beginning of the current second frozen.
    pub async fn freezeSecondFor<T>(
        &mut self,
        callback: impl AsyncFnOnce(&mut TestApp, Date) -> TimeResult<T>,
    ) -> TimeResult<T> {
        let time = self.current_second().await?;
        self.travelToFor(time, callback).await
    }

    async fn current_time(&mut self) -> TimeResult<Date> {
        self.boot().await?;
        Ok(application_scope(self.app().container_handle(), async { Date::now(None) }).await?)
    }

    async fn current_second(&mut self) -> TimeResult<Date> {
        let now = self.current_time().await?;
        let instant = now.asDateTime();
        let whole_second =
            Timestamp::from_second(crate::support::timestamp_seconds(instant.timestamp()))
                .map_err(|_| DateError::OutOfRange)?;
        Ok(Date::fromUtc(whole_second, instant.time_zone().clone()))
    }
}

/// Relative test time travel from the fixture's current clock.
pub struct Wormhole<'a> {
    app: &'a mut TestApp,
    amount: i64,
}

macro_rules! travel_units {
    ($plural:ident, $singular:ident, $field:ident, $scale:literal) => {
        pub async fn $plural(self) -> TimeResult<Date> {
            let value = self
                .amount
                .checked_mul($scale)
                .ok_or(DateError::OutOfRange)?;
            self.shift(DateInterval {
                $field: value,
                ..DateInterval::default()
            })
            .await
        }

        pub async fn $singular(self) -> TimeResult<Date> {
            self.$plural().await
        }
    };
}

impl Wormhole<'_> {
    travel_units!(microseconds, microsecond, microseconds, 1);
    travel_units!(milliseconds, millisecond, microseconds, 1_000);
    travel_units!(seconds, second, microseconds, 1_000_000);
    travel_units!(minutes, minute, microseconds, 60_000_000);
    travel_units!(hours, hour, microseconds, 3_600_000_000);
    travel_units!(days, day, days, 1);
    travel_units!(weeks, week, days, 7);
    travel_units!(months, month, months, 1);
    travel_units!(years, year, years, 1);

    async fn shift(self, interval: DateInterval) -> TimeResult<Date> {
        let time = self
            .app
            .current_time()
            .await?
            .plus(interval, MonthOverflow::Carry)?;
        self.app.travelTo(time.clone());
        Ok(time)
    }
}
