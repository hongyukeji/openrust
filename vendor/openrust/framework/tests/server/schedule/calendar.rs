//! 验证 cron 计划变更、有限补跑与共享数据库协调。
#![cfg(all(feature = "queue", any(feature = "db-postgres", feature = "db-mysql")))]

use chrono::{TimeZone, Utc};
use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::config::FrameworkConfig;
use openrust::core::{App, BoxFuture, Result as FrameworkResult};
use openrust::database::{
    DatabaseServiceProvider, Db,
    sea_orm::Statement,
    sea_orm_migration::{MigrationTrait, MigratorTrait},
};
use openrust::queue::{Job, JobContext, QueueMigration, QueueServiceProvider};
use openrust::schedule::{CronOptions, MissedRunPolicy, Scheduler};
use sea_orm::ConnectionTrait;
use sha2::Digest;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct CalendarMigrator;

impl MigratorTrait for CalendarMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(QueueMigration)]
    }
}

#[derive(serde::Deserialize)]
struct CalendarJob;

impl Job for CalendarJob {
    const NAME: &'static str = "tests.schedule-calendar.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

async fn build_app(
    url: &str,
    key: &str,
    expression: &str,
    time_zone: &str,
    missed_runs: MissedRunPolicy,
) -> FrameworkResult<App> {
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let queue = QueueServiceProvider::new();
    queue.contributions().register_job::<CalendarJob>()?;
    queue.contributions().schedule_cron(
        key,
        "schedule-calendar-tests",
        CalendarJob::NAME,
        &serde_json::json!({}),
        expression,
        CronOptions {
            time_zone: time_zone.to_owned(),
            missed_runs,
        },
    )?;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider).provider(queue);
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

#[tokio::test]
async fn cron_plan_revisions_are_separate_and_shared_ticks_dispatch_once()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping cron database integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<CalendarMigrator>(&admin_url).await?;
    let key = "tests.schedule-calendar.v1";
    let app = build_app(
        database.url(),
        key,
        "* * * * *",
        "UTC",
        MissedRunPolicy::Skip,
    )
    .await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    let ready = format!("/tmp/openrust-calendar-ready-{}", std::process::id());
    let result_path = format!("/tmp/openrust-calendar-result-{}", std::process::id());
    let current = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let start_at = if current % 60 < 55 {
        current + 2
    } else {
        (current / 60 + 1) * 60 + 1
    };
    let start_at_text = start_at.to_string();
    let _ = std::fs::remove_file(&ready);
    let _ = std::fs::remove_file(&result_path);
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "child_calendar_scheduler_ticks_due_slot",
            "--nocapture",
        ])
        .env("OPENRUST_CRON_CHILD_URL", database.url())
        .env("OPENRUST_CRON_READY_FILE", &ready)
        .env("OPENRUST_CRON_START_AT", &start_at_text)
        .env("OPENRUST_CRON_RESULT_FILE", &result_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    wait_for_file(&ready).await?;
    wait_until_epoch(start_at).await?;
    let parent_scheduler = scheduler.clone();
    let parent_tick = tokio::spawn(async move { parent_scheduler.tick().await });
    let child_status = tokio::task::spawn_blocking(move || child.wait()).await??;
    let parent_report = parent_tick.await??;
    let child_result = std::fs::read_to_string(&result_path)?;
    std::fs::remove_file(&ready)?;
    std::fs::remove_file(&result_path)?;
    assert!(child_status.success());
    let (child_dispatched, child_already) = parse_child_report(&child_result)?;
    assert_eq!(parent_report.dispatched + child_dispatched, 1);
    assert_eq!(parent_report.already_dispatched + child_already, 1);

    // 表达式和时区变化分别生成新版本身份，旧版本水位不会抑制新计划。
    let revised = build_app(
        database.url(),
        key,
        "*/1 * * * *",
        "UTC",
        MissedRunPolicy::Skip,
    )
    .await?;
    let revised_report = revised.state().resolve::<Scheduler>().await?.tick().await?;
    assert_eq!(revised_report.dispatched, 1);
    let revised_time_zone = build_app(
        database.url(),
        key,
        "* * * * *",
        "Asia/Tokyo",
        MissedRunPolicy::Skip,
    )
    .await?;
    let time_zone_report = revised_time_zone
        .state()
        .resolve::<Scheduler>()
        .await?
        .tick()
        .await?;
    assert_eq!(time_zone_report.dispatched, 1);

    let db = app.state().resolve::<Db>().await?;
    assert_eq!(database_count(&db, "schedule_runs").await?, 3);
    assert_eq!(database_count(&db, "jobs").await?, 3);
    db.connection()?.close().await?;
    drop(scheduler);
    drop(revised);
    drop(revised_time_zone);
    drop(app);
    drop(db);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn child_calendar_scheduler_ticks_due_slot() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_CRON_CHILD_URL") else {
        return Ok(());
    };
    let app = build_app(
        &url,
        "tests.schedule-calendar.v1",
        "* * * * *",
        "UTC",
        MissedRunPolicy::Skip,
    )
    .await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    std::fs::write(std::env::var("OPENRUST_CRON_READY_FILE")?, b"ready")?;
    wait_until_epoch(std::env::var("OPENRUST_CRON_START_AT")?.parse()?).await?;
    let report = scheduler.tick().await?;
    std::fs::write(
        std::env::var("OPENRUST_CRON_RESULT_FILE")?,
        format!("{}:{}", report.dispatched, report.already_dispatched),
    )?;
    Ok(())
}

#[tokio::test]
async fn cron_dst_gap_and_fold_slots_are_persisted_correctly()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping cron DST database integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<CalendarMigrator>(&admin_url).await?;

    let spring_key = "tests.schedule-calendar-spring.v1";
    let spring_expression = "30 2 * * *";
    let spring_revision = schedule_revision(spring_expression, "America/New_York");
    let spring = build_app(
        database.url(),
        spring_key,
        spring_expression,
        "America/New_York",
        MissedRunPolicy::CatchUp { max_slots: 1 },
    )
    .await?;
    let spring_scheduler = spring.state().resolve::<Scheduler>().await?;
    let spring_db = spring.state().resolve::<Db>().await?;
    let spring_watermark = Utc
        .with_ymd_and_hms(2024, 3, 10, 6, 0, 0)
        .single()
        .ok_or("invalid spring DST fixture")?
        .timestamp();
    let spring_seed = format!("c{spring_revision}-seed");
    insert_seed_marker(&spring_db, spring_key, &spring_seed, spring_watermark).await?;
    let spring_report = spring_scheduler.tick().await?;
    assert_eq!(spring_report.dispatched, 1);
    assert!(spring_report.has_deferred);
    assert_eq!(
        schedule_slot_count(
            &spring_db,
            spring_key,
            &format!("c{spring_revision}-20240310T0230")
        )
        .await?,
        0
    );
    assert_eq!(
        schedule_slot_count(
            &spring_db,
            spring_key,
            &format!("c{spring_revision}-20240311T0230")
        )
        .await?,
        1
    );
    spring_db.connection()?.close().await?;
    drop(spring_scheduler);
    drop(spring_db);
    drop(spring);

    let fall_key = "tests.schedule-calendar-fall.v1";
    let fall_expression = "30 1 * * *";
    let fall_revision = schedule_revision(fall_expression, "America/New_York");
    let fall = build_app(
        database.url(),
        fall_key,
        fall_expression,
        "America/New_York",
        MissedRunPolicy::CatchUp { max_slots: 1 },
    )
    .await?;
    let fall_scheduler = fall.state().resolve::<Scheduler>().await?;
    let fall_db = fall.state().resolve::<Db>().await?;
    let fall_watermark = Utc
        .with_ymd_and_hms(2024, 11, 3, 5, 0, 0)
        .single()
        .ok_or("invalid fall DST fixture")?
        .timestamp();
    let fall_seed = format!("c{fall_revision}-seed");
    insert_seed_marker(&fall_db, fall_key, &fall_seed, fall_watermark).await?;
    let first_fold = fall_scheduler.tick().await?;
    assert_eq!(first_fold.dispatched, 1);
    assert!(first_fold.has_deferred);
    let repeated_wall_slot = format!("c{fall_revision}-20241103T0130");
    assert_eq!(
        schedule_slot_count(&fall_db, fall_key, &repeated_wall_slot).await?,
        1
    );
    let after_fold = fall_scheduler.tick().await?;
    assert_eq!(after_fold.dispatched, 1);
    assert_eq!(
        schedule_slot_count(&fall_db, fall_key, &repeated_wall_slot).await?,
        1
    );

    fall_db.connection()?.close().await?;
    drop(fall_scheduler);
    drop(fall_db);
    drop(fall);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn cron_catch_up_is_bounded_per_tick_and_continues_backlog()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping cron catch-up integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<CalendarMigrator>(&admin_url).await?;
    let key = "tests.schedule-calendar.v1";
    let expression = "0 * * * *";
    let revision = schedule_revision(expression, "UTC");
    let app = build_app(
        database.url(),
        key,
        expression,
        "UTC",
        MissedRunPolicy::CatchUp { max_slots: 2 },
    )
    .await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    let db = app.state().resolve::<Db>().await?;
    let seed_time = database_clock_seconds(&db).await? - 26 * 60 * 60;
    let seed_slot = format!("c{revision}-seed");
    insert_seed_marker(&db, key, &seed_slot, seed_time).await?;
    let seeded_watermark = schedule_run_time(&db, key, &seed_slot).await?;
    let current_time = database_clock_seconds(&db).await?;
    let expected =
        usize::try_from(current_time.div_euclid(3600) - seeded_watermark.div_euclid(3600))?;
    assert!(
        expected >= 25,
        "fixture must span a cross-day scheduler outage"
    );

    let mut reports = Vec::new();
    for _ in 0..32 {
        let report = scheduler.tick().await?;
        assert!(report.dispatched <= 2);
        reports.push(report);
        if !report.has_deferred {
            break;
        }
    }
    assert!(reports.iter().any(|report| report.has_deferred));
    assert!(reports.last().is_some_and(|report| !report.has_deferred));
    assert_eq!(
        reports
            .iter()
            .map(|report| report.dispatched)
            .sum::<usize>(),
        expected
    );
    assert_eq!(
        database_count(&db, "schedule_runs").await?,
        i64::try_from(expected + 1)?
    );
    assert_eq!(database_count(&db, "jobs").await?, i64::try_from(expected)?);

    db.connection()?.close().await?;
    drop(scheduler);
    drop(db);
    drop(app);
    database.close().await?;
    Ok(())
}

async fn database_count(db: &Db, logical_table: &str) -> FrameworkResult<i64> {
    let backend = db.backend()?;
    let table = db.quoted_table_name(logical_table)?;
    let row = db
        .connection()?
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT COUNT(*) AS count FROM {table}"),
        ))
        .await
        .map_err(openrust::Error::internal)?
        .ok_or_else(|| {
            openrust::Error::custom("test_count_missing", "count query returned no row")
        })?;
    row.try_get("", "count").map_err(openrust::Error::internal)
}

fn schedule_revision(expression: &str, time_zone: &str) -> String {
    let mut digest = sha2::Sha256::new();
    digest.update(expression.as_bytes());
    digest.update([0]);
    digest.update(time_zone.as_bytes());
    let digest = digest.finalize();
    format!(
        "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]
    )
}

async fn insert_seed_marker(
    db: &Db,
    key: &str,
    slot: &str,
    scheduled_at: i64,
) -> FrameworkResult<()> {
    let backend = db.backend()?;
    let table = db.quoted_table_name("schedule_runs")?;
    let (sql, values) = match backend {
        openrust::database::DbBackend::Postgres => (
            format!(
                "INSERT INTO {table} (schedule_key, slot_key, scheduled_at, triggered_job_id, created_at) VALUES ($1, $2, to_timestamp($3), $4, CURRENT_TIMESTAMP)"
            ),
            vec![
                key.to_owned().into(),
                slot.to_owned().into(),
                scheduled_at.into(),
                "seed-job".into(),
            ],
        ),
        openrust::database::DbBackend::MySql => (
            format!(
                "INSERT INTO {table} (schedule_key, slot_key, scheduled_at, triggered_job_id, created_at) VALUES (?, ?, FROM_UNIXTIME(?), ?, UTC_TIMESTAMP(6))"
            ),
            vec![
                key.to_owned().into(),
                slot.to_owned().into(),
                scheduled_at.into(),
                "seed-job".into(),
            ],
        ),
        _ => {
            return Err(openrust::Error::custom(
                "unsupported_test_backend",
                "calendar test requires PostgreSQL or MySQL",
            ));
        }
    };
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(openrust::Error::internal)?;
    Ok(())
}

async fn database_clock_seconds(db: &Db) -> FrameworkResult<i64> {
    let backend = db.backend()?;
    let sql = match backend {
        openrust::database::DbBackend::Postgres => {
            "SELECT FLOOR(EXTRACT(EPOCH FROM CURRENT_TIMESTAMP))::BIGINT AS now"
        }
        openrust::database::DbBackend::MySql => {
            "SELECT CAST(FLOOR(UNIX_TIMESTAMP(UTC_TIMESTAMP(6))) AS SIGNED) AS now"
        }
        _ => {
            return Err(openrust::Error::custom(
                "unsupported_test_backend",
                "calendar test requires PostgreSQL or MySQL",
            ));
        }
    };
    let row = db
        .connection()?
        .query_one_raw(Statement::from_string(backend, sql))
        .await
        .map_err(openrust::Error::internal)?
        .ok_or_else(|| {
            openrust::Error::custom("test_clock_missing", "database returned no clock")
        })?;
    row.try_get("", "now").map_err(openrust::Error::internal)
}

async fn schedule_run_time(db: &Db, key: &str, slot: &str) -> FrameworkResult<i64> {
    let backend = db.backend()?;
    let table = db.quoted_table_name("schedule_runs")?;
    let sql = match backend {
        openrust::database::DbBackend::Postgres => format!(
            "SELECT FLOOR(EXTRACT(EPOCH FROM scheduled_at))::BIGINT AS scheduled_at FROM {table} WHERE schedule_key = $1 AND slot_key = $2"
        ),
        openrust::database::DbBackend::MySql => format!(
            "SELECT CAST(UNIX_TIMESTAMP(scheduled_at) AS SIGNED) AS scheduled_at FROM {table} WHERE schedule_key = ? AND slot_key = ?"
        ),
        _ => {
            return Err(openrust::Error::custom(
                "unsupported_test_backend",
                "calendar test requires PostgreSQL or MySQL",
            ));
        }
    };
    let row = db
        .connection()?
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            vec![key.to_owned().into(), slot.to_owned().into()],
        ))
        .await
        .map_err(openrust::Error::internal)?
        .ok_or_else(|| {
            openrust::Error::custom(
                "test_schedule_marker_missing",
                "seeded schedule marker was not found",
            )
        })?;
    row.try_get("", "scheduled_at")
        .map_err(openrust::Error::internal)
}

async fn schedule_slot_count(db: &Db, key: &str, slot: &str) -> FrameworkResult<i64> {
    let backend = db.backend()?;
    let table = db.quoted_table_name("schedule_runs")?;
    let sql = match backend {
        openrust::database::DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS count FROM {table} WHERE schedule_key = $1 AND slot_key = $2"
        ),
        openrust::database::DbBackend::MySql => {
            format!("SELECT COUNT(*) AS count FROM {table} WHERE schedule_key = ? AND slot_key = ?")
        }
        _ => {
            return Err(openrust::Error::custom(
                "unsupported_test_backend",
                "calendar test requires PostgreSQL or MySQL",
            ));
        }
    };
    let row = db
        .connection()?
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            vec![key.to_owned().into(), slot.to_owned().into()],
        ))
        .await
        .map_err(openrust::Error::internal)?
        .ok_or_else(|| {
            openrust::Error::custom("test_count_missing", "count query returned no row")
        })?;
    row.try_get("", "count").map_err(openrust::Error::internal)
}

fn parse_child_report(report: &str) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let (dispatched, already_dispatched) = report
        .split_once(':')
        .ok_or("child scheduler report is malformed")?;
    Ok((dispatched.parse()?, already_dispatched.parse()?))
}

async fn wait_for_file(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !std::path::Path::new(path).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

async fn wait_until_epoch(seconds: u64) -> Result<(), Box<dyn std::error::Error>> {
    let target = UNIX_EPOCH + Duration::from_secs(seconds);
    if let Ok(delay) = target.duration_since(SystemTime::now()) {
        tokio::time::sleep(delay).await;
    }
    Ok(())
}
