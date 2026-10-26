//! Database-coordinated UTC job scheduling.

use crate::core::{BoxFuture, Error, Result};
use crate::db::{DatabaseTransaction, Db, DbBackend};
use crate::queue::Queue;
use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use serde_json::Value;
use std::time::Duration;

const MAX_INTERVAL_SECS: u64 = 2_592_000;
const DAY_SECS: u64 = 86_400;

/// A fixed UTC schedule cadence.
#[derive(Debug, Clone, Copy)]
pub enum ScheduleFrequency {
    /// Trigger once per interval slot, including the current slot when first registered.
    Interval(Duration),
    /// Trigger once at the specified UTC hour and minute, skipping missed daily slots.
    DailyAtUtc {
        /// Hour in the UTC day, from 0 through 23.
        hour: u8,
        /// Minute in the UTC hour, from 0 through 59.
        minute: u8,
    },
}

impl ScheduleFrequency {
    pub(crate) fn validate(self) -> Result<()> {
        match self {
            Self::Interval(interval)
                if interval.is_zero()
                    || interval > Duration::from_secs(MAX_INTERVAL_SECS)
                    || interval.subsec_nanos() != 0 =>
            {
                Err(Error::custom(
                    "invalid_schedule_interval",
                    "schedule interval must be whole seconds in 1..=2592000",
                ))
            }
            Self::DailyAtUtc { hour, minute } if hour > 23 || minute > 59 => Err(Error::custom(
                "invalid_daily_schedule_time",
                "daily UTC time must use hour 0..=23 and minute 0..=59",
            )),
            _ => Ok(()),
        }
    }

    fn due_slot(self, now: u64) -> Option<(String, u64)> {
        match self {
            Self::Interval(interval) => {
                let seconds = interval.as_secs();
                let slot = now / seconds;
                Some((slot.to_string(), slot * seconds))
            }
            Self::DailyAtUtc { hour, minute } => {
                let today = now / DAY_SECS * DAY_SECS;
                let slot = today + u64::from(hour) * 3600 + u64::from(minute) * 60;
                (now >= slot).then(|| (today.to_string(), slot))
            }
        }
    }
}

/// One explicitly registered scheduler entry.
#[derive(Debug, Clone)]
pub struct ScheduleDefinition {
    key: String,
    queue: String,
    job_name: String,
    payload: Value,
    frequency: ScheduleFrequency,
}

impl ScheduleDefinition {
    pub(crate) fn new(
        key: String,
        queue: String,
        job_name: String,
        payload: Value,
        frequency: ScheduleFrequency,
    ) -> Self {
        Self {
            key,
            queue,
            job_name,
            payload,
            frequency,
        }
    }

    /// Stable key used for cross-process slot deduplication.
    pub fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn job_name(&self) -> &str {
        &self.job_name
    }

    fn frequency_label(&self) -> String {
        match self.frequency {
            ScheduleFrequency::Interval(interval) => {
                format!("every {}s", interval.as_secs())
            }
            ScheduleFrequency::DailyAtUtc { hour, minute } => {
                format!("daily at {hour:02}:{minute:02} UTC")
            }
        }
    }
}

/// Result of one scheduler scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScheduleReport {
    /// Number of registered entries with a due slot.
    pub due: usize,
    /// Number of new jobs committed.
    pub dispatched: usize,
    /// Number of due slots already handled by another scheduler instance.
    pub already_dispatched: usize,
}

/// Runs registered schedules and atomically deduplicates slots through the shared database.
#[derive(Clone)]
pub struct Scheduler {
    db: Db,
    queue: Queue,
    entries: Vec<ScheduleDefinition>,
    config: crate::config::QueueConfig,
}

impl Scheduler {
    pub(crate) fn new(
        db: Db,
        queue: Queue,
        entries: Vec<ScheduleDefinition>,
        config: crate::config::QueueConfig,
    ) -> Self {
        Self {
            db,
            queue,
            entries,
            config,
        }
    }

    /// Returns the schedule entries registered by application plugins.
    pub fn entries(&self) -> &[ScheduleDefinition] {
        &self.entries
    }

    /// Dispatches each due schedule slot once across all processes sharing the database.
    pub async fn tick(&self) -> Result<ScheduleReport> {
        let now = database_unix_time(&self.db).await?;
        let mut report = ScheduleReport::default();
        for entry in &self.entries {
            let Some((slot, scheduled_at)) = entry.frequency.due_slot(now) else {
                continue;
            };
            report.due += 1;
            let connection = self.db.connection()?;
            let transaction = connection.begin().await.map_err(Error::internal)?;
            let job_id = self
                .queue
                .dispatch_in_transaction(
                    &transaction,
                    &entry.queue,
                    &entry.job_name,
                    &entry.payload,
                )
                .await?;
            if insert_schedule_slot(&self.db, &transaction, entry, &slot, scheduled_at, &job_id)
                .await?
            {
                transaction.commit().await.map_err(Error::internal)?;
                report.dispatched += 1;
            } else {
                transaction.rollback().await.map_err(Error::internal)?;
                report.already_dispatched += 1;
            }
        }
        Ok(report)
    }

    /// Repeatedly scans schedules until Ctrl-C or SIGTERM, then exits after the current scan.
    pub async fn run(&self) -> Result<()> {
        loop {
            self.tick().await?;
            tokio::select! {
                signal = shutdown_signal() => {
                    signal?;
                    return Ok(());
                }
                () = tokio::time::sleep(Duration::from_millis(self.config.poll_interval_millis)) => {}
            }
        }
    }
}

async fn database_unix_time(db: &Db) -> Result<u64> {
    let backend = db.backend()?;
    let sql = match backend {
        DbBackend::Postgres => "SELECT FLOOR(EXTRACT(EPOCH FROM CURRENT_TIMESTAMP))::BIGINT AS now",
        DbBackend::MySql => "SELECT CAST(FLOOR(UNIX_TIMESTAMP(UTC_TIMESTAMP(6))) AS SIGNED) AS now",
        _ => {
            return Err(Error::custom(
                "unsupported_schedule_backend",
                "database scheduler requires PostgreSQL or MySQL",
            ));
        }
    };
    let row = db
        .connection()?
        .query_one_raw(Statement::from_string(backend, sql))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::custom("schedule_clock_unavailable", "database returned no clock"))?;
    let unix: i64 = row.try_get("", "now").map_err(Error::internal)?;
    u64::try_from(unix).map_err(Error::internal)
}

async fn insert_schedule_slot(
    db: &Db,
    transaction: &DatabaseTransaction,
    entry: &ScheduleDefinition,
    slot: &str,
    scheduled_at: u64,
    job_id: &str,
) -> Result<bool> {
    let table = db.quoted_table_name("schedule_runs")?;
    let backend = db.backend()?;
    let sql = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (schedule_key, slot_key, scheduled_at, triggered_job_id, created_at) VALUES ($1, $2, to_timestamp($3), $4, CURRENT_TIMESTAMP) ON CONFLICT (schedule_key, slot_key) DO NOTHING"
        ),
        DbBackend::MySql => format!(
            "INSERT IGNORE INTO {table} (schedule_key, slot_key, scheduled_at, triggered_job_id, created_at) VALUES (?, ?, FROM_UNIXTIME(?), ?, UTC_TIMESTAMP(6))"
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_schedule_backend",
                "database scheduler requires PostgreSQL or MySQL",
            ));
        }
    };
    let result = transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            sql,
            vec![
                entry.key.clone().into(),
                slot.into(),
                scheduled_at.into(),
                job_id.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(result.rows_affected() == 1)
}

pub(crate) struct ScheduleRunCommand;

impl crate::core::Command for ScheduleRunCommand {
    fn name(&self) -> &'static str {
        "schedule run"
    }

    fn about(&self) -> &'static str {
        "Dispatch due schedule entries"
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(
        &'a self,
        app: &'a crate::core::App,
        args: Vec<String>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let scheduler = app.state().try_get::<Scheduler>()?;
            match args.as_slice() {
                [once] if once == "--once" => {
                    scheduler.tick().await?;
                    Ok(())
                }
                [] => scheduler.run().await,
                _ => Err(Error::custom(
                    "invalid_schedule_command",
                    "usage: schedule run [--once]",
                )),
            }
        })
    }
}

pub(crate) struct ScheduleListCommand;

impl crate::core::Command for ScheduleListCommand {
    fn name(&self) -> &'static str {
        "schedule list"
    }

    fn about(&self) -> &'static str {
        "List registered schedule entries"
    }

    fn accepts_args(&self) -> bool {
        false
    }

    #[allow(clippy::print_stdout)]
    fn run<'a>(
        &'a self,
        app: &'a crate::core::App,
        _args: Vec<String>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let scheduler = app.state().try_get::<Scheduler>()?;
            for entry in scheduler.entries() {
                println!(
                    "{} {} {} {}",
                    entry.key,
                    entry.queue,
                    entry.job_name,
                    entry.frequency_label()
                );
            }
            Ok(())
        })
    }
}

#[cfg(unix)]
async fn shutdown_signal() -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate()).map_err(Error::internal)?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.map_err(Error::internal),
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> Result<()> {
    tokio::signal::ctrl_c().await.map_err(Error::internal)
}

#[cfg(test)]
mod tests {
    use super::ScheduleFrequency;
    use std::time::Duration;

    #[test]
    fn interval_slots_are_stable_and_daily_slots_use_utc_day() {
        let interval = ScheduleFrequency::Interval(Duration::from_secs(60));
        assert_eq!(interval.due_slot(119), Some(("1".into(), 60)));

        let daily = ScheduleFrequency::DailyAtUtc {
            hour: 3,
            minute: 15,
        };
        assert_eq!(daily.due_slot(10_000), None);
        assert_eq!(daily.due_slot(12_000), Some(("0".into(), 11_700)));
        assert_eq!(daily.due_slot(86_300), Some(("0".into(), 11_700)));
        assert_eq!(daily.due_slot(86_400), None);
    }

    #[test]
    fn frequency_bounds_reject_fractional_and_out_of_range_values() {
        assert!(
            ScheduleFrequency::Interval(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(
            ScheduleFrequency::Interval(Duration::from_nanos(1))
                .validate()
                .is_err()
        );
        assert!(
            ScheduleFrequency::Interval(Duration::from_secs(2_592_001))
                .validate()
                .is_err()
        );
        assert!(
            ScheduleFrequency::DailyAtUtc {
                hour: 24,
                minute: 0
            }
            .validate()
            .is_err()
        );
    }
}
