//! 由共享数据库时钟协调的间隔、UTC daily 与 cron 任务调度。

use crate::core::{BoxFuture, Error, Result};
use crate::db::{DatabaseTransaction, Db, DbBackend};
use crate::queue::Queue;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron_parser::Schedule as CronSchedule;
use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use serde_json::Value;
use sha2::Digest;
use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

const MAX_INTERVAL_SECS: u64 = 2_592_000;
const DAY_SECS: u64 = 86_400;
const MAX_CATCH_UP_SLOTS: usize = 100;
// 回拨最多可重复一天的本地分钟；额外扫描预算避免旧槽挤掉后续新槽。
const MAX_REPEATED_LOCAL_MINUTES: usize = 1_440;

/// cron 调度的时区与错过触发策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronOptions {
    /// IANA 时区名称；默认使用 `UTC`。
    pub time_zone: String,
    /// scheduler 停机期间错过触发的处理方式。
    pub missed_runs: MissedRunPolicy,
}

impl Default for CronOptions {
    fn default() -> Self {
        Self {
            time_zone: "UTC".to_owned(),
            missed_runs: MissedRunPolicy::Skip,
        }
    }
}

/// cron 调度错过触发的处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissedRunPolicy {
    /// 只派发当前分钟匹配的触发；停机期间的旧触发不补跑。
    Skip,
    /// 按时间顺序补跑，每次 tick 最多派发指定数量，范围为 1 到 100。
    CatchUp {
        /// 单次 tick 的最大补跑触发数。
        max_slots: usize,
    },
}

/// 固定间隔、UTC daily 或带时区的 cron 调度周期。
#[derive(Debug, Clone)]
pub enum ScheduleFrequency {
    /// 每个 interval 时段触发一次；首次注册时包含当前时段。
    Interval(Duration),
    /// 每日指定 UTC 小时和分钟触发一次，并跳过错过的日期时段。
    DailyAtUtc {
        /// UTC 日内小时，范围为 0 到 23。
        hour: u8,
        /// UTC 小时内分钟，范围为 0 到 59。
        minute: u8,
    },
    /// 按五字段 cron 表达式和 IANA 时区触发。
    Cron(CronScheduleConfig),
}

/// 调度器内部持有的已编译 cron 计划；应用通过 `QueueContributions::schedule_cron` 创建。
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct CronScheduleConfig {
    expression: String,
    parsed_schedule: CronSchedule,
    time_zone: Tz,
    missed_runs: MissedRunPolicy,
    revision: String,
}

impl ScheduleFrequency {
    pub(crate) fn cron(
        expression: &str,
        time_zone_name: &str,
        missed_runs: MissedRunPolicy,
    ) -> Result<Self> {
        let parsed_schedule = CronSchedule::parse(expression).map_err(|error| {
            Error::custom(
                "invalid_schedule_cron",
                format!("invalid five-field cron expression: {error}"),
            )
        })?;
        let time_zone = Tz::from_str(time_zone_name).map_err(|error| {
            Error::custom(
                "invalid_schedule_time_zone",
                format!("invalid IANA time zone `{time_zone_name}`: {error}"),
            )
        })?;
        let mut digest = sha2::Sha256::new();
        digest.update(expression.as_bytes());
        digest.update([0]);
        digest.update(time_zone_name.as_bytes());
        let revision =
            digest.finalize()[..8]
                .iter()
                .fold(String::with_capacity(16), |mut value, byte| {
                    let _ = write!(value, "{byte:02x}");
                    value
                });
        Ok(Self::Cron(CronScheduleConfig {
            expression: expression.to_owned(),
            parsed_schedule,
            time_zone,
            missed_runs,
            revision,
        }))
    }

    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Interval(interval)
                if interval.is_zero()
                    || *interval > Duration::from_secs(MAX_INTERVAL_SECS)
                    || interval.subsec_nanos() != 0 =>
            {
                Err(Error::custom(
                    "invalid_schedule_interval",
                    "schedule interval must be whole seconds in 1..=2592000",
                ))
            }
            Self::DailyAtUtc { hour, minute } if *hour > 23 || *minute > 59 => Err(Error::custom(
                "invalid_daily_schedule_time",
                "daily UTC time must use hour 0..=23 and minute 0..=59",
            )),
            Self::Cron(CronScheduleConfig {
                missed_runs: MissedRunPolicy::CatchUp { max_slots },
                ..
            }) if !(1..=MAX_CATCH_UP_SLOTS).contains(max_slots) => Err(Error::custom(
                "invalid_schedule_catch_up_limit",
                "cron catch-up max_slots must be in 1..=100",
            )),
            _ => Ok(()),
        }
    }

    fn due_slot(&self, now: u64) -> Option<(String, u64)> {
        match self {
            Self::Interval(interval) => {
                let seconds = interval.as_secs();
                let slot = now / seconds;
                Some((slot.to_string(), slot * seconds))
            }
            Self::DailyAtUtc { hour, minute } => {
                let today = now / DAY_SECS * DAY_SECS;
                let slot = today + u64::from(*hour) * 3600 + u64::from(*minute) * 60;
                (now >= slot).then(|| (today.to_string(), slot))
            }
            Self::Cron(_) => None,
        }
    }
}

/// 一个显式注册的调度项。
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

    /// 用于跨进程时段去重的稳定键。
    pub fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn job_name(&self) -> &str {
        &self.job_name
    }

    fn frequency_label(&self) -> String {
        match &self.frequency {
            ScheduleFrequency::Interval(interval) => {
                format!("every {}s", interval.as_secs())
            }
            ScheduleFrequency::DailyAtUtc { hour, minute } => {
                format!("daily at {hour:02}:{minute:02} UTC")
            }
            ScheduleFrequency::Cron(config) => {
                format!("cron `{}` ({})", config.expression, config.time_zone)
            }
        }
    }
}

/// 一次调度扫描的结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScheduleReport {
    /// 已到触发时段的已注册项数量。
    pub due: usize,
    /// 已提交的新任务数量。
    pub dispatched: usize,
    /// 已由其他 scheduler 实例处理的到期时段数量。
    pub already_dispatched: usize,
    /// 是否还有因补跑上限而留待后续扫描处理的触发。
    pub has_deferred: bool,
}

/// 运行已注册的调度项，并通过共享数据库原子去重时段。
#[derive(Clone)]
pub struct Scheduler {
    db: Db,
    queue: Queue,
    entries: Vec<ScheduleDefinition>,
    live_contributions: Option<Arc<crate::queue::QueueContributions>>,
    config: crate::config::QueueConfig,
    scopes: crate::core::container::ScopeFactory,
}

struct TriggerScope(Arc<crate::core::container::Scope>);
impl Drop for TriggerScope {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl Scheduler {
    pub(crate) fn new(
        db: Db,
        queue: Queue,
        entries: Vec<ScheduleDefinition>,
        config: crate::config::QueueConfig,
        scopes: crate::core::container::ScopeFactory,
    ) -> Self {
        Self {
            db,
            queue,
            entries,
            live_contributions: None,
            config,
            scopes,
        }
    }

    pub(crate) fn with_live_contributions(
        mut self,
        contributions: Arc<crate::queue::QueueContributions>,
    ) -> Self {
        self.live_contributions = Some(contributions);
        self
    }

    /// 返回应用插件注册的调度项。
    pub fn entries(&self) -> Vec<ScheduleDefinition> {
        self.live_contributions
            .as_ref()
            .map_or_else(|| self.entries.clone(), |items| items.schedules())
    }

    /// 在所有共享该数据库的进程中，对每个到期调度时段仅派发一次。
    ///
    /// 每个时段的事务派发使用独立作用域及空执行元数据，不继承调用者的租户或请求；
    /// 后续队列任务由 worker 为每次尝试另建作用域。当前调度项不声明租户。
    /// Scheduler 由服务提供方构造，作用域工厂引用同一个应用容器。
    pub async fn tick(&self) -> Result<ScheduleReport> {
        let now = database_unix_time(&self.db).await?;
        let mut report = ScheduleReport::default();
        for entry in self.entries() {
            let (slots, deferred) = due_slots(&self.db, &entry, now).await?;
            if !slots.is_empty() {
                report.due += 1;
            }
            report.has_deferred |= deferred;
            for (slot, scheduled_at) in slots {
                let scope = TriggerScope(Arc::new(self.scopes.scope()?));
                let result = scope
                    .0
                    .run(crate::tenant::TenantContext::without_current(
                        crate::core::with_execution_context(
                            crate::core::ExecutionContext::new(
                                crate::core::ExecutionMetadata::default(),
                                None,
                                crate::core::Cancellation::new(),
                            ),
                            async {
                                let connection = self.db.connection()?;
                                let transaction =
                                    connection.begin().await.map_err(Error::internal)?;
                                let job_id = self
                                    .queue
                                    .dispatch_in_transaction(
                                        &transaction,
                                        &entry.queue,
                                        &entry.job_name,
                                        &entry.payload,
                                    )
                                    .await?;
                                if insert_schedule_slot(
                                    &self.db,
                                    &transaction,
                                    &entry,
                                    &slot,
                                    scheduled_at,
                                    &job_id,
                                )
                                .await?
                                {
                                    transaction.commit().await.map_err(Error::internal)?;
                                    Ok(true)
                                } else {
                                    transaction.rollback().await.map_err(Error::internal)?;
                                    Ok(false)
                                }
                            },
                        ),
                    ))
                    .await;
                let cleanup = scope
                    .0
                    .shutdown(
                        tokio::time::Instant::now()
                            + Duration::from_secs(self.config.shutdown_timeout_secs),
                    )
                    .await;
                let dispatched = match (result, cleanup) {
                    (Ok(value), Ok(())) => value,
                    (Err(error), Ok(())) | (Ok(_), Err(error)) => return Err(error),
                    (Err(error), Err(_)) => {
                        tracing::warn!("scheduler trigger scope cleanup failed");
                        return Err(error);
                    }
                };
                if dispatched {
                    report.dispatched += 1;
                } else {
                    report.already_dispatched += 1;
                }
            }
        }
        Ok(report)
    }

    /// 持续扫描调度，直到收到 Ctrl-C 或 SIGTERM；当前扫描完成后退出。
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

async fn due_slots(
    db: &Db,
    entry: &ScheduleDefinition,
    now: u64,
) -> Result<(Vec<(String, u64)>, bool)> {
    let ScheduleFrequency::Cron(config) = &entry.frequency else {
        return Ok((entry.frequency.due_slot(now).into_iter().collect(), false));
    };
    let CronScheduleConfig {
        parsed_schedule: schedule,
        time_zone,
        missed_runs,
        revision,
        ..
    } = config;
    let now_i64 = i64::try_from(now).map_err(Error::internal)?;
    let minute = now_i64 - now_i64.rem_euclid(60);
    let now_utc = DateTime::<Utc>::from_timestamp(minute, 0).ok_or_else(|| {
        Error::custom(
            "schedule_clock_out_of_range",
            "database time is outside the supported calendar range",
        )
    })?;
    let slots = match missed_runs {
        MissedRunPolicy::Skip => {
            let slots = if schedule.includes(&now_utc.with_timezone(time_zone)) {
                vec![cron_slot(&now_utc, *time_zone, revision)?]
            } else {
                Vec::new()
            };
            (slots, false)
        }
        MissedRunPolicy::CatchUp { max_slots } => {
            let watermark = schedule_watermark(db, entry, revision).await?;
            let Some(watermark) = watermark else {
                let slots = if schedule.includes(&now_utc.with_timezone(time_zone)) {
                    vec![cron_slot(&now_utc, *time_zone, revision)?]
                } else {
                    Vec::new()
                };
                return Ok((slots, false));
            };
            let cursor = DateTime::<Utc>::from_timestamp(watermark, 0).ok_or_else(|| {
                Error::custom(
                    "invalid_schedule_watermark",
                    "stored schedule time is outside the supported calendar range",
                )
            })?;
            let mut seen = HashSet::new();
            let mut candidates = Vec::new();
            for occurrence in schedule
                .after(&cursor.with_timezone(time_zone))
                .take(max_slots + MAX_REPEATED_LOCAL_MINUTES + 1)
            {
                let utc = occurrence.with_timezone(&Utc);
                if utc.timestamp() > minute {
                    break;
                }
                let slot = cron_slot(&utc, *time_zone, revision)?;
                if seen.insert(slot.0.clone()) {
                    candidates.push(slot);
                }
            }
            let persisted = existing_schedule_slots(db, entry, &candidates).await?;
            candidates.retain(|(slot, _)| !persisted.contains(slot));
            let deferred = candidates.len() > *max_slots;
            candidates.truncate(*max_slots);
            (candidates, deferred)
        }
    };
    Ok(slots)
}

async fn existing_schedule_slots(
    db: &Db,
    entry: &ScheduleDefinition,
    candidates: &[(String, u64)],
) -> Result<HashSet<String>> {
    if candidates.is_empty() {
        return Ok(HashSet::new());
    }
    let backend = db.backend()?;
    let table = db.quoted_table_name("schedule_runs")?;
    let mut values = Vec::with_capacity(candidates.len() + 1);
    values.push(entry.key.clone().into());
    values.extend(candidates.iter().map(|(slot, _)| slot.clone().into()));
    let placeholders = (0..candidates.len())
        .map(|index| match backend {
            DbBackend::Postgres => format!("${}", index + 2),
            DbBackend::MySql => "?".to_owned(),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let schedule_placeholder = if backend == DbBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    let sql = format!(
        "SELECT slot_key FROM {table} WHERE schedule_key = {schedule_placeholder} AND slot_key IN ({placeholders})"
    );
    let rows = db
        .connection()?
        .query_all_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?;
    rows.into_iter()
        .map(|row| row.try_get("", "slot_key").map_err(Error::internal))
        .collect()
}

fn cron_slot(utc: &DateTime<Utc>, time_zone: Tz, revision: &str) -> Result<(String, u64)> {
    let local = utc.with_timezone(&time_zone).format("%Y%m%dT%H%M");
    let timestamp = u64::try_from(utc.timestamp()).map_err(Error::internal)?;
    Ok((format!("c{revision}-{local}"), timestamp))
}

async fn schedule_watermark(
    db: &Db,
    entry: &ScheduleDefinition,
    revision: &str,
) -> Result<Option<i64>> {
    let backend = db.backend()?;
    let table = db.quoted_table_name("schedule_runs")?;
    let sql = match backend {
        DbBackend::Postgres => format!(
            "SELECT MAX(FLOOR(EXTRACT(EPOCH FROM scheduled_at)))::BIGINT AS watermark FROM {table} WHERE schedule_key = $1 AND slot_key LIKE $2"
        ),
        DbBackend::MySql => format!(
            "SELECT CAST(MAX(UNIX_TIMESTAMP(scheduled_at)) AS SIGNED) AS watermark FROM {table} WHERE schedule_key = ? AND slot_key LIKE ?"
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_schedule_backend",
                "database scheduler requires PostgreSQL or MySQL",
            ));
        }
    };
    let row = db
        .connection()?
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            vec![entry.key.clone().into(), format!("c{revision}-%").into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| {
            Error::custom(
                "schedule_watermark_unavailable",
                "database returned no schedule watermark row",
            )
        })?;
    row.try_get("", "watermark").map_err(Error::internal)
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
            let scheduler = app.state().resolve::<Scheduler>().await?;
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
            let scheduler = app.state().resolve::<Scheduler>().await?;
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
mod provider_command_tests {
    use super::*;
    use crate::core::{Command, ProviderBuilder, RegisterContext, ServiceProvider};
    use std::sync::Arc;

    #[tokio::test]
    async fn schedule_list_resolves_factory_scheduler() -> Result<()> {
        struct SchedulerProvider;
        impl ServiceProvider for SchedulerProvider {
            fn register<'a>(
                &'a self,
                app: &'a mut RegisterContext<'_>,
            ) -> crate::core::BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.singleton::<Scheduler, _>(|resolver| {
                        Box::pin(async move {
                            let db = crate::db::Db::default();
                            Ok(Arc::new(Scheduler::new(
                                db.clone(),
                                crate::queue::Queue::new(db),
                                Vec::new(),
                                crate::config::QueueConfig::default(),
                                resolver.scope_factory(),
                            )))
                        })
                    })
                    .await?;
                    Ok(())
                })
            }
        }
        let mut builder = ProviderBuilder::new();
        builder.provider(SchedulerProvider);
        let app =
            crate::core::App::from_provider_runtime(builder.start(figment::Figment::new()).await?)?;
        ScheduleListCommand.run(&app, Vec::new()).await
    }
}

#[cfg(test)]
mod tests {
    use super::{MissedRunPolicy, ScheduleFrequency, cron_slot};
    use chrono::{TimeZone, Utc};
    use chrono_tz::America::New_York;
    use cron_parser::Schedule;
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

    #[test]
    fn cron_skips_nonexistent_spring_minutes_and_deduplicates_repeated_fall_minutes()
    -> Result<(), Box<dyn std::error::Error>> {
        let spring = Schedule::parse("30 2 * * *")?;
        let spring_start = New_York
            .with_ymd_and_hms(2024, 3, 10, 0, 0, 0)
            .single()
            .ok_or("invalid New York spring transition fixture")?;
        let spring_next = spring
            .next_after(&spring_start)
            .ok_or("spring schedule has no next occurrence")?;
        assert_eq!(
            spring_next.format("%Y-%m-%d %H:%M").to_string(),
            "2024-03-11 02:30"
        );

        let fall = Schedule::parse("30 1 * * *")?;
        let fall_start = New_York
            .with_ymd_and_hms(2024, 11, 3, 0, 0, 0)
            .single()
            .ok_or("invalid New York fall transition fixture")?;
        let occurrences = fall.after(&fall_start).take(2).collect::<Vec<_>>();
        assert_eq!(occurrences.len(), 2);
        assert_eq!(
            occurrences[1].timestamp() - occurrences[0].timestamp(),
            3600
        );
        let first_occurrence = occurrences
            .first()
            .ok_or("fall schedule did not return its first occurrence")?;
        let second_occurrence = occurrences
            .get(1)
            .ok_or("fall schedule did not return its second occurrence")?;
        let first = cron_slot(
            &first_occurrence.with_timezone(&Utc),
            New_York,
            "0123456789abcdef",
        )?;
        let second = cron_slot(
            &second_occurrence.with_timezone(&Utc),
            New_York,
            "0123456789abcdef",
        )?;
        assert_eq!(first.0, second.0);
        assert_ne!(first.1, second.1);
        Ok(())
    }

    #[test]
    fn cron_calendar_handles_leap_days_and_bounded_catch_up_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        let leap_day = Schedule::parse("0 0 29 2 *")?;
        let start = Utc
            .with_ymd_and_hms(2024, 2, 28, 0, 0, 0)
            .single()
            .ok_or("invalid leap-day fixture")?;
        let next = leap_day
            .next_after(&start)
            .ok_or("leap-day schedule has no next occurrence")?;
        assert_eq!(next.format("%Y-%m-%d").to_string(), "2024-02-29");

        let valid = ScheduleFrequency::cron(
            "* * * * *",
            "UTC",
            MissedRunPolicy::CatchUp { max_slots: 100 },
        )?;
        assert!(valid.validate().is_ok());
        let invalid = ScheduleFrequency::cron(
            "* * * * *",
            "UTC",
            MissedRunPolicy::CatchUp { max_slots: 101 },
        )?;
        assert!(invalid.validate().is_err());
        assert!(
            ScheduleFrequency::cron("* * * * *", "Mars/Olympus", MissedRunPolicy::Skip).is_err()
        );
        assert!(ScheduleFrequency::cron("0 0 31 2 *", "UTC", MissedRunPolicy::Skip).is_err());
        Ok(())
    }
}
