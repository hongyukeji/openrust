//! 生产模块的真实数据库时钟扫描、跨 App 时段去重及 heartbeat worker 生命周期。
//! 两个同进程 App 共享隔离数据库，不作为多 OS 进程集群证据。
#![cfg(all(
    feature = "auth",
    feature = "queue",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::jobs::{HeartbeatRun, example_heartbeat::ExampleHeartbeatJob};
use figment::{Figment, providers::Serialized};
use futures_util::FutureExt as _;
use openrust::Db;
use openrust::db::sea_orm::{ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::queue::{Job, Queue};
use openrust::schedule::Scheduler;
use openrust::testing::{TestApp, TestDatabase};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const BUDGET: Duration = Duration::from_secs(5);
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
#[derive(Default)]
struct AttemptProbe {
    constructed: AtomicU64,
    closed: Mutex<BTreeSet<u64>>,
}
impl AttemptProbe {
    fn closed(&self) -> TestResult<BTreeSet<u64>> {
        Ok(self
            .closed
            .lock()
            .map_err(|_| "heartbeat probe lock poisoned")?
            .clone())
    }
}
async fn application(
    url: &str,
    probe: Arc<AttemptProbe>,
) -> TestResult<TestApp<crate::bootstrap::Server>> {
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"}, "database":{"url":url},
            "queue":{"driver":"database"},
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"scheduler-business","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        })));
    let mut builder = TestApp::<crate::bootstrap::Server>::builder(source);
    // 不替换生产任务、调度器或队列；仅通过公开测试 hook 为现业务 scoped 依赖加观测。
    builder.configure_after_register(move |bindings| {
        let probe = Arc::clone(&probe);
        bindings.scoped::<HeartbeatRun, _>(move |resolver| {
            let probe = Arc::clone(&probe);
            Box::pin(async move {
                let id = probe.constructed.fetch_add(1, Ordering::SeqCst) + 1;
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        let inserted = probe
                            .closed
                            .lock()
                            .map_err(|_| {
                                openrust::Error::custom(
                                    "heartbeat_probe_lock_failed",
                                    "heartbeat probe unavailable",
                                )
                            })?
                            .insert(id);
                        if !inserted {
                            return Err(openrust::Error::custom(
                                "heartbeat_cleanup_repeated",
                                "heartbeat cleanup repeated",
                            ));
                        }
                        Ok(())
                    })
                })?;
                Ok(Arc::new(HeartbeatRun { id }))
            })
        });
        Ok(())
    });
    Ok(builder.build().await?)
}
async fn rows(db: &Db, sql: String) -> TestResult<Vec<openrust::db::sea_orm::QueryResult>> {
    Ok(db
        .primary_connection()?
        .query_all_raw(Statement::from_string(db.backend()?, sql))
        .await?)
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "调度竞争、持久任务与尝试回收须在同一隔离生命周期中验证"
)]
async fn production_heartbeat_scheduler_deduplicates_slots_and_worker_closes_each_attempt()
-> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping real module scheduler: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&admin).await?;
    let probe = Arc::new(AttemptProbe::default());
    let first = application(database.url(), Arc::clone(&probe)).await;
    let mut first = match first {
        Ok(app) => app,
        Err(error) => {
            let _ = database.close().await;
            return Err(error);
        }
    };
    let second = application(database.url(), Arc::clone(&probe)).await;
    let mut second = match second {
        Ok(app) => app,
        Err(error) => {
            let _ = first.shutdown(tokio::time::Instant::now() + BUDGET).await;
            let _ = database.close().await;
            return Err(error);
        }
    };
    let result = std::panic::AssertUnwindSafe(Box::pin(async {
        let a = first.app()?.state();
        let b = second.app()?.state();
        let scheduler_a = a.resolve::<Scheduler>().await?;
        let scheduler_b = b.resolve::<Scheduler>().await?;
        assert!(!Arc::ptr_eq(&scheduler_a, &scheduler_b));
        assert!(
            scheduler_a
                .entries()
                .iter()
                .any(|entry| entry.key() == "example-heartbeat")
        );
        assert_eq!(probe.constructed.load(Ordering::SeqCst), 0);
        // tick 使用数据库时钟；三次扫描在同一个有界窗口内完成，允许跨过分钟边界。
        let reports = Box::pin(tokio::time::timeout(BUDGET, async {
            let (left, right) = tokio::join!(scheduler_a.tick(), scheduler_b.tick());
            Ok::<_, openrust::Error>([left?, right?, scheduler_a.tick().await?])
        }))
        .await
        .map_err(|_| "real scheduler ticks exceeded deadline")??;
        let db = a.resolve::<Db>().await?;
        let runs_table = db.quoted_table_name("schedule_runs")?;
        let jobs_table = db.quoted_table_name("jobs")?;
        let runs = rows(
            &db,
            format!(
                "SELECT schedule_key,slot_key,triggered_job_id FROM {runs_table} ORDER BY slot_key"
            ),
        )
        .await?;
        assert!(!runs.is_empty());
        // 60 秒 interval 在 5 秒扫描预算内至多出现两个真实时段；不伪造调度时钟。
        assert!(runs.len() <= 2);
        let mut slots = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for row in &runs {
            assert_eq!(
                row.try_get::<String>("", "schedule_key")?,
                "example-heartbeat"
            );
            assert!(slots.insert(row.try_get::<String>("", "slot_key")?));
            assert!(ids.insert(row.try_get::<String>("", "triggered_job_id")?));
        }
        assert_eq!(
            reports
                .iter()
                .map(|report| report.dispatched)
                .sum::<usize>(),
            runs.len()
        );
        assert!(
            reports
                .iter()
                .map(|report| report.already_dispatched)
                .sum::<usize>()
                >= 1
        );
        let pending = rows(
            &db,
            format!("SELECT id,queue,job_name,payload FROM {jobs_table}"),
        )
        .await?;
        assert_eq!(pending.len(), runs.len());
        for row in &pending {
            assert!(ids.contains(&row.try_get::<String>("", "id")?));
            assert_eq!(row.try_get::<String>("", "queue")?, "example");
            assert_eq!(
                row.try_get::<String>("", "job_name")?,
                ExampleHeartbeatJob::NAME
            );
            assert_eq!(
                row.try_get::<serde_json::Value>("", "payload")?,
                serde_json::Value::Null
            );
        }
        let queue_a = a.resolve::<Queue>().await?;
        let queue_b = b.resolve::<Queue>().await?;
        for index in 0..pending.len() {
            let (queue, state) = if index % 2 == 0 {
                (&queue_a, a)
            } else {
                (&queue_b, b)
            };
            assert!(
                tokio::time::timeout(BUDGET, queue.run_next_with_state("example", state)).await??
            );
        }
        assert!(!tokio::time::timeout(BUDGET, queue_a.run_next_with_state("example", a)).await??);
        let completed = u64::try_from(pending.len())?;
        assert_eq!(probe.constructed.load(Ordering::SeqCst), completed);
        assert_eq!(probe.closed()?, (1..=completed).collect());
        assert!(
            rows(&db, format!("SELECT id FROM {jobs_table}"))
                .await?
                .is_empty()
        );
        let failed = db.quoted_table_name("failed_jobs")?;
        assert!(
            rows(&db, format!("SELECT id FROM {failed}"))
                .await?
                .is_empty()
        );
        Ok::<u64, Box<dyn std::error::Error>>(completed)
    }))
    .catch_unwind()
    .await;
    // 两个生产 App 均显式等待关闭，即使业务断言/查询返回错误也尝试回收，保留首错。
    let close_a = first.shutdown(tokio::time::Instant::now() + BUDGET).await;
    let close_b = second.shutdown(tokio::time::Instant::now() + BUDGET).await;
    let close_database = database.close().await;
    let completed = match result {
        Ok(result) => result?,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    close_a?;
    close_b?;
    close_database?;
    assert_eq!(probe.constructed.load(Ordering::SeqCst), completed);
    assert_eq!(probe.closed()?, (1..=completed).collect());
    Ok(())
}
