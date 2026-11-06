//! 专用 SQLite 迁移所有者在入口取消和 runtime 退出后的真实 DDL 生命周期。

use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::db::{
    DatabaseBackend, DatabaseInstanceConfig, Db, sqlite::migration_lock::SqliteMigrationLock,
};
use crate::{Error, Result};
use sea_orm::ConnectionTrait;
use tokio::sync::{Notify, oneshot};

const TEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONTENTION_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Default)]
struct NativeGate {
    entered: Notify,
    notified: AtomicBool,
    open: Mutex<bool>,
    changed: Condvar,
}

impl NativeGate {
    // 只能从 SQLite 原生 progress callback 调用；超时返回 false 中断 SQL，避免永久挂起。
    fn progress(&self) -> bool {
        let Ok(mut open) = self.open.lock() else {
            return false;
        };
        if !self.notified.swap(true, Ordering::SeqCst) {
            self.entered.notify_one();
        }
        let deadline = Instant::now() + TEST_TIMEOUT;
        while !*open {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            let Ok((next, timeout)) = self.changed.wait_timeout(open, remaining) else {
                return false;
            };
            open = next;
            if timeout.timed_out() && !*open {
                return false;
            }
        }
        true
    }

    fn release(&self) {
        if let Ok(mut open) = self.open.lock() {
            *open = true;
            self.changed.notify_all();
        }
    }
}

struct Fixture {
    path: Option<tempfile::TempPath>,
    directory: Option<tempfile::TempDir>,
    db: Db,
    url: String,
    gate: Arc<NativeGate>,
    contender: Option<Db>,
}

impl Fixture {
    async fn new() -> Result<Self> {
        // into_temp_path 先关闭创建文件的 FD；主池打开后不再由测试额外开闭主文件。
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = tempfile::NamedTempFile::new_in(directory.path())
            .map_err(Error::internal)?
            .into_temp_path();
        let uri = url::Url::from_file_path(&path)
            .map_err(|()| Error::custom("test_path_invalid", "临时数据库路径无效"))?;
        let url = uri.as_str().replacen("file:", "sqlite:", 1);
        let config = DatabaseInstanceConfig {
            max_connections: 2,
            min_connections: 2,
            ..Default::default()
        };
        let connection = crate::db::connect_database(
            &url,
            &config,
            DatabaseBackend::Sqlite,
            "migration ownership test",
            None,
            None,
        )
        .await?;
        let db = Db::default();
        db.initialize(connection)?;
        Ok(Self {
            path: Some(path),
            directory: Some(directory),
            db,
            url,
            gate: Arc::new(NativeGate::default()),
            contender: None,
        })
    }

    fn work(
        &self,
    ) -> (
        impl std::future::Future<Output = Result<()>> + Send + 'static,
        oneshot::Receiver<Result<()>>,
    ) {
        let db = self.db.clone();
        let url = self.url.clone();
        let gate = self.gate.clone();
        let (finished, receiver) = oneshot::channel();
        let work = async move {
            let result = gated_ddl(db, url, gate).await;
            let _ = finished.send(result);
            Ok(())
        };
        (work, receiver)
    }

    async fn assert_contended(&self) -> Result<()> {
        match SqliteMigrationLock::acquire(
            self.contender.as_ref().unwrap_or(&self.db),
            &self.url,
            CONTENTION_TIMEOUT,
        )
        .await
        {
            Err(error) => assert_eq!(error.code(), "migration_lock_timeout"),
            Ok(lock) => {
                lock.release()?;
                return Err(Error::custom(
                    "test_migration_lock_released_early",
                    "原生 DDL 完成前迁移锁被提前释放",
                ));
            }
        }
        Ok(())
    }

    async fn verify_and_close(mut self) -> Result<()> {
        let lock = SqliteMigrationLock::acquire(&self.db, &self.url, TEST_TIMEOUT).await?;
        let connection = self.db.connection()?;
        let row = connection
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT count(*) AS n FROM openrust_owner_probe",
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("test_ddl_row_missing", "实际 DDL 表不存在"))?;
        assert_eq!(row.try_get::<i64>("", "n").map_err(Error::internal)?, 100);
        lock.release()?;
        tokio::time::timeout(TEST_TIMEOUT, connection.close())
            .await
            .map_err(|_| Error::custom("test_pool_close_timeout", "测试连接池关闭超时"))?
            .map_err(Error::internal)?;
        // 整个池与迁移所有者结束后，回收本 fixture 自有目录及其 sidecar。
        self.path.take();
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.gate.release();
        let Some(path) = self.path.take() else {
            return;
        };
        let Ok(connection) = self.db.connection() else {
            std::mem::forget((path, self.directory.take()));
            return;
        };
        // 断言失败/取消也不先删除活动 SQLite 文件；清理不依赖正在退出的测试 runtime。
        let contender = self.contender.take().and_then(|db| db.connection().ok());
        let retained = Arc::new(Mutex::new(Some((path, self.directory.take()))));
        let worker_retained = retained.clone();
        let worker = std::thread::Builder::new()
            .name("sqlite-owner-test-cleanup".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(runtime) = runtime {
                    let closed = runtime.block_on(async {
                        tokio::time::timeout(TEST_TIMEOUT, async {
                            if let Some(contender) = contender {
                                contender.close().await?;
                            }
                            connection.close().await
                        })
                        .await
                    });
                    if matches!(closed, Ok(Ok(()))) {
                        return;
                    }
                }
                std::mem::forget(worker_retained);
            });
        if worker.is_err() {
            std::mem::forget(retained);
        }
    }
}

async fn gated_ddl(db: Db, url: String, gate: Arc<NativeGate>) -> Result<()> {
    eprintln!("sqlite-helper phase=acquire_lock");
    let lock = SqliteMigrationLock::acquire(&db, &url, TEST_TIMEOUT)
        .await
        .map_err(|error| helper_phase_error("test_helper_lock_failed", error))?;
    eprintln!("sqlite-helper phase=lock_acquired");
    let outcome: Result<()> = async {
        let database = db.connection().map_err(|error| helper_phase_error("test_helper_db_failed", error))?;
        let pool = database.get_sqlite_connection_pool();
        let mut connection = pool.acquire().await.map_err(|error| helper_phase_error("test_helper_pool_failed", error))?;
        eprintln!("sqlite-helper phase=pool_acquired");
        {
            let mut handle = connection.lock_handle().await.map_err(|error| helper_phase_error("test_helper_handle_failed", error))?;
            handle.set_progress_handler(1, move || gate.progress());
        }
        eprintln!("sqlite-helper phase=progress_handler_installed");
        // CREATE TABLE AS 的 VM 确实在 SQLite 原生 worker 上执行；gate 不只是 Rust future 通知。
        let sql_result = sqlx::query(
            "CREATE TABLE openrust_owner_probe AS WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM seq WHERE n < 100) SELECT n FROM seq",
        ).execute(&mut *connection).await.map_err(|error| helper_phase_error("test_helper_sql_failed", error));
        eprintln!("sqlite-helper phase=ddl_returned");
        {
            let mut handle = connection.lock_handle().await.map_err(|error| helper_phase_error("test_helper_handle_failed", error))?;
            handle.remove_progress_handler();
        }
        // 显式等待归还，专用 owner runtime 不能在 pool 的归还任务前退出。
        connection.return_to_pool().await;
        sql_result?;
        Ok(())
    }.await;
    let release = lock.release();
    outcome?;
    release
}

fn helper_phase_error(code: &'static str, error: impl std::error::Error) -> Error {
    eprintln!("sqlite-helper phase={code} error={error:?}");
    let mut source = error.source();
    while let Some(error) = source {
        eprintln!("sqlite-helper phase={code} source={error:?}");
        source = error.source();
    }
    Error::custom(code, error.to_string())
}

async fn entered_or_finished(
    gate: &NativeGate,
    finished: &mut oneshot::Receiver<Result<()>>,
    owner: &mut oneshot::Receiver<Result<()>>,
) -> Result<()> {
    tokio::time::timeout(TEST_TIMEOUT, async {
        tokio::select! {
            () = gate.entered.notified() => Ok(()),
            result = finished => premature_gate_result(result, "test_work_finished_before_native_gate"),
            result = owner => premature_gate_result(result, "test_owner_finished_before_native_gate"),
        }
    }).await.map_err(|_| Error::custom("test_native_gate_timeout", "原生gate、work结果与owner结果均未到达"))?
}

fn premature_gate_result(
    outcome: std::result::Result<Result<()>, oneshot::error::RecvError>,
    code: &'static str,
) -> Result<()> {
    match outcome {
        Ok(Err(error)) => Err(helper_phase_error(code, error)),
        Ok(Ok(())) => Err(Error::custom(
            code,
            "真实SQLite所有者在native gate前已成功结束",
        )),
        Err(error) => Err(helper_phase_error(code, error)),
    }
}

async fn entered(gate: &NativeGate) -> Result<()> {
    tokio::time::timeout(TEST_TIMEOUT, gate.entered.notified())
        .await
        .map_err(|_| Error::custom("test_native_gate_timeout", "未进入 SQLite 原生 DDL gate"))
}

async fn completed(receiver: oneshot::Receiver<Result<()>>) -> Result<()> {
    tokio::time::timeout(TEST_TIMEOUT, receiver)
        .await
        .map_err(|_| Error::custom("test_owner_timeout", "迁移所有者未完成"))?
        .map_err(|_| Error::custom("test_owner_lost", "迁移所有者退出且未报告"))?
}

#[tokio::test]
async fn aborting_wrapper_keeps_lock_until_native_ddl_and_release_complete() -> Result<()> {
    let fixture = Fixture::new().await?;
    let (work, mut finished) = fixture.work();
    let (owner_finished, mut owner_result) = oneshot::channel();
    let wrapper = tokio::spawn(async move {
        let outcome = super::sqlite_owner(
            None,
            tokio::time::Instant::now() + TEST_TIMEOUT,
            uncancelled(),
            Box::pin(work),
        )
        .await;
        let _ = owner_finished.send(outcome);
    });
    entered_or_finished(&fixture.gate, &mut finished, &mut owner_result).await?;
    wrapper.abort();
    let cancelled = tokio::time::timeout(TEST_TIMEOUT, wrapper)
        .await
        .map_err(|_| Error::custom("test_wrapper_abort_timeout", "包装任务未结束"))?;
    assert!(cancelled.is_err_and(|error| error.is_cancelled()));
    fixture.assert_contended().await?;
    fixture.gate.release();
    completed(finished).await?;
    fixture.verify_and_close().await
}

#[tokio::test]
async fn exiting_original_runtime_keeps_dedicated_owner_and_native_ddl_alive() -> Result<()> {
    let fixture = Fixture::new().await?;
    let (work, mut finished) = fixture.work();
    let (owner_finished, mut owner_result) = oneshot::channel();
    let (exit, exit_requested) = oneshot::channel::<()>();
    let (exited, runtime_exited) = oneshot::channel::<Result<()>>();
    let original = std::thread::Builder::new()
        .name("sqlite-original-test-runtime".into())
        .spawn(move || {
            let outcome = (|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(Error::internal)?;
                runtime.block_on(async {
                    tokio::spawn(async move {
                        let outcome = super::sqlite_owner(
                            None,
                            tokio::time::Instant::now() + TEST_TIMEOUT,
                            uncancelled(),
                            Box::pin(work),
                        )
                        .await;
                        let _ = owner_finished.send(outcome);
                    });
                    let _ = exit_requested.await;
                });
                drop(runtime);
                Ok(())
            })();
            let _ = exited.send(outcome);
        })
        .map_err(Error::internal)?;
    entered_or_finished(&fixture.gate, &mut finished, &mut owner_result).await?;
    exit.send(())
        .map_err(|()| Error::custom("test_original_runtime_lost", "原包装 runtime 已退出"))?;
    completed(runtime_exited).await?;
    original
        .join()
        .map_err(|_| Error::custom("test_original_runtime_panicked", "原包装 runtime 线程异常"))?;
    fixture.assert_contended().await?;
    fixture.gate.release();
    completed(finished).await?;
    fixture.verify_and_close().await
}

// 与 helper 所有权测试分开：以下测试由真实 CatalogMigrator/perform 决定事务和锁释放。
#[derive(Default)]
struct ArmedGate {
    armed: AtomicBool,
    rollback_armed: AtomicBool,
    native: Arc<NativeGate>,
    rollback: Arc<NativeGate>,
}

struct ReleaseGateOnDrop(Arc<NativeGate>);

impl Drop for ReleaseGateOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

tokio::task_local! {
    static PRODUCTION_GATE: Arc<ArmedGate>;
}

#[derive(Clone, Copy)]
enum ProductionCase {
    Fail,
    Panic,
    Current,
    Next,
    Recovery,
}

struct ProductionMigration(ProductionCase);

impl sea_orm_migration::MigrationName for ProductionMigration {
    fn name(&self) -> &'static str {
        match self.0 {
            ProductionCase::Fail => "m20261102_000001_production_failure",
            ProductionCase::Panic => "m20261102_000001_production_panic",
            ProductionCase::Current => "m20261102_000001_production_current",
            ProductionCase::Next => "m20261102_000002_production_next",
            ProductionCase::Recovery => "m20261102_000003_production_recovery",
        }
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl sea_orm_migration::MigrationTrait for ProductionMigration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(
        &self,
        manager: &sea_orm_migration::SchemaManager,
    ) -> std::result::Result<(), sea_orm::DbErr> {
        let gate = PRODUCTION_GATE
            .try_with(Arc::clone)
            .map_err(|_| sea_orm::DbErr::Custom("生产迁移测试缺少 task-local gate".into()))?;
        let gated = matches!(
            self.0,
            ProductionCase::Fail | ProductionCase::Panic | ProductionCase::Current
        );
        if gated {
            gate.armed.store(true, Ordering::SeqCst);
        }
        let sql = match self.0 {
            ProductionCase::Fail => {
                "CREATE TABLE production_failed_probe AS WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM seq WHERE n < 100) SELECT n FROM seq"
            }
            ProductionCase::Panic => {
                "CREATE TABLE production_panic_probe AS WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM seq WHERE n < 100) SELECT n FROM seq"
            }
            ProductionCase::Current => {
                "CREATE TABLE production_current_probe AS WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM seq WHERE n < 100) SELECT n FROM seq"
            }
            ProductionCase::Next => "CREATE TABLE production_next_probe (id INTEGER)",
            ProductionCase::Recovery => "CREATE TABLE production_recovery_probe (id INTEGER)",
        };
        // 使用生产 SchemaManager 传入的 ORM transaction，测试不手工提交/回滚或释放锁。
        let result = manager.get_connection().execute_unprepared(sql).await;
        gate.armed.store(false, Ordering::SeqCst);
        result?;
        match self.0 {
            ProductionCase::Fail => {
                // 返回Err后的下一次原生VM操作应为生产rollback；独立gate证明锁未提前释放。
                gate.rollback_armed.store(true, Ordering::SeqCst);
                Err(sea_orm::DbErr::Custom(
                    "controlled production migration error".into(),
                ))
            }
            ProductionCase::Panic => {
                // 与资源回收测试相同，明确注入 unwind，保留真实异常清理路径。
                std::panic::resume_unwind(Box::new("controlled production migration unwind"))
            }
            _ => Ok(()),
        }
    }

    async fn down(
        &self,
        _: &sea_orm_migration::SchemaManager,
    ) -> std::result::Result<(), sea_orm::DbErr> {
        Err(sea_orm::DbErr::Custom("本回归只执行 up".into()))
    }
}

fn failing_migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
    vec![Box::new(ProductionMigration(ProductionCase::Fail))]
}

fn panicking_migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
    vec![Box::new(ProductionMigration(ProductionCase::Panic))]
}

fn cancelling_migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
    vec![
        Box::new(ProductionMigration(ProductionCase::Current)),
        Box::new(ProductionMigration(ProductionCase::Next)),
    ]
}

fn recovery_migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
    vec![Box::new(ProductionMigration(ProductionCase::Recovery))]
}

type MigrationFactory = fn() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>>;

fn catalog(factory: MigrationFactory) -> super::super::CatalogMigrator {
    super::super::CatalogMigrator {
        sources: vec![super::super::MigrationEntry {
            module: "sqlite-production-ownership-test",
            names: factory()
                .iter()
                .map(|migration| migration.name().to_owned())
                .collect(),
            migrations: factory,
        }],
    }
}

fn uncancelled() -> super::MigrationCancellation {
    super::MigrationCancellation {
        caller: crate::core::Cancellation::new(),
        shutdown: crate::core::Cancellation::new(),
    }
}

impl Fixture {
    async fn install_production_gate(&mut self) -> Result<Arc<ArmedGate>> {
        let gate = Arc::new(ArmedGate {
            native: self.gate.clone(),
            ..Default::default()
        });
        // 竞争者使用预先打开且未安装progress hook的独立标准pool；否则其identity PRAGMA
        // 也可能被同一个armed hook阻塞，把连接等待误判为真实文件锁竞争。
        let config = DatabaseInstanceConfig {
            max_connections: 1,
            min_connections: 1,
            ..Default::default()
        };
        let competitor = crate::db::connect_database(
            &self.url,
            &config,
            DatabaseBackend::Sqlite,
            "migration lock contender test",
            None,
            None,
        )
        .await?;
        let contender = Db::default();
        contender.initialize(competitor)?;
        self.contender = Some(contender);
        let database = self.db.connection()?;
        let pool = database.get_sqlite_connection_pool();
        // 同时借出两条连接，避免两次设置实际命中同一physical handle。
        let mut first = pool.acquire().await.map_err(Error::internal)?;
        let mut second = pool.acquire().await.map_err(Error::internal)?;
        for connection in [&mut first, &mut second] {
            let callback = gate.clone();
            let mut handle = connection.lock_handle().await.map_err(Error::internal)?;
            handle.set_progress_handler(1, move || {
                if callback.rollback_armed.load(Ordering::SeqCst) {
                    callback.rollback.progress()
                } else {
                    !callback.armed.load(Ordering::SeqCst) || callback.native.progress()
                }
            });
        }
        // 此处只是测试前置安装hook；生产迁移的归还/rollback/release完全由perform管理。
        first.return_to_pool().await;
        second.return_to_pool().await;
        Ok(gate)
    }

    fn production_work(
        &self,
        gate: Arc<ArmedGate>,
        factory: MigrationFactory,
        cancellation: super::MigrationCancellation,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        let db = self.db.clone();
        let url = self.url.clone();
        async move {
            PRODUCTION_GATE
                .scope(gate, async move {
                    super::perform(
                        &db,
                        &url,
                        &catalog(factory),
                        super::super::MigrationAction::Up(None),
                        TEST_TIMEOUT,
                        &cancellation,
                    )
                    .await
                })
                .await
        }
    }

    async fn table_exists(&self, table: &str) -> Result<bool> {
        let connection = self.db.connection()?;
        sea_orm_migration::SchemaManager::new(&connection)
            .has_table(table)
            .await
            .map_err(Error::internal)
    }

    async fn history_count(&self) -> Result<i64> {
        let row = self
            .db
            .connection()?
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT count(*) AS n FROM openrust_migrations",
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("test_history_missing", "迁移历史查询无结果"))?;
        row.try_get("", "n").map_err(Error::internal)
    }

    async fn reopen(&mut self) -> Result<()> {
        let config = DatabaseInstanceConfig {
            max_connections: 2,
            min_connections: 2,
            ..Default::default()
        };
        let connection = crate::db::connect_database(
            &self.url,
            &config,
            DatabaseBackend::Sqlite,
            "migration recovery test",
            None,
            None,
        )
        .await?;
        let db = Db::default();
        db.initialize(connection)?;
        self.db = db;
        Ok(())
    }

    async fn close_production(mut self) -> Result<()> {
        if let Some(contender) = self.contender.take() {
            tokio::time::timeout(TEST_TIMEOUT, contender.connection()?.close())
                .await
                .map_err(|_| Error::custom("test_pool_close_timeout", "竞争连接池关闭超时"))?
                .map_err(Error::internal)?;
        }
        tokio::time::timeout(TEST_TIMEOUT, self.db.connection()?.close())
            .await
            .map_err(|_| Error::custom("test_pool_close_timeout", "测试连接池关闭超时"))?
            .map_err(Error::internal)?;
        self.path.take();
        Ok(())
    }
}

async fn owner_result(wrapper: tokio::task::JoinHandle<Result<()>>) -> Result<Result<()>> {
    tokio::time::timeout(TEST_TIMEOUT, wrapper)
        .await
        .map_err(|_| Error::custom("test_production_owner_timeout", "生产迁移所有者未结束"))?
        .map_err(|_| Error::custom("test_production_owner_lost", "生产迁移包装任务异常"))
}

async fn run_recovery(fixture: &Fixture, gate: Arc<ArmedGate>) -> Result<()> {
    let cancellation = uncancelled();
    let work = fixture.production_work(gate, recovery_migrations, cancellation.clone());
    super::sqlite_owner(
        None,
        tokio::time::Instant::now() + TEST_TIMEOUT,
        cancellation,
        Box::pin(work),
    )
    .await?;
    assert!(fixture.table_exists("production_recovery_probe").await?);
    Ok(())
}

#[tokio::test]
async fn production_transaction_error_rolls_back_ddl_and_history_before_next_migration()
-> Result<()> {
    let mut fixture = Fixture::new().await?;
    let gate = fixture.install_production_gate().await?;
    let _rollback_cleanup = ReleaseGateOnDrop(gate.rollback.clone());
    let cancellation = uncancelled();
    let work = fixture.production_work(gate.clone(), failing_migrations, cancellation.clone());
    let wrapper = tokio::spawn(super::sqlite_owner(
        None,
        tokio::time::Instant::now() + TEST_TIMEOUT,
        cancellation,
        Box::pin(work),
    ));
    entered(&fixture.gate).await?;
    fixture.assert_contended().await?;
    fixture.gate.release();
    entered(&gate.rollback).await?;
    // 若事务仅Drop排队rollback而perform已release，此处会错误取得锁。
    fixture.assert_contended().await?;
    assert!(!wrapper.is_finished());
    gate.rollback.release();
    assert!(owner_result(wrapper).await?.is_err());
    assert!(!fixture.table_exists("production_failed_probe").await?);
    assert_eq!(fixture.history_count().await?, 0);
    // 下一次真实perform成功证明前次已rollback并释放锁，而非只收到Rust错误。
    run_recovery(&fixture, gate).await?;
    assert_eq!(fixture.history_count().await?, 1);
    fixture.close_production().await
}

#[tokio::test]
async fn production_migration_panic_closes_pool_and_new_pool_can_reacquire_lock() -> Result<()> {
    let mut fixture = Fixture::new().await?;
    let gate = fixture.install_production_gate().await?;
    let cancellation = uncancelled();
    let work = fixture.production_work(gate, panicking_migrations, cancellation.clone());
    let wrapper = tokio::spawn(super::sqlite_owner(
        None,
        tokio::time::Instant::now() + TEST_TIMEOUT,
        cancellation,
        Box::pin(work),
    ));
    entered(&fixture.gate).await?;
    fixture.assert_contended().await?;
    fixture.gate.release();
    let outcome = owner_result(wrapper).await?;
    assert_eq!(
        outcome.err().map(|error| error.code().to_owned()),
        Some("migration_panicked".to_owned())
    );
    assert!(
        fixture
            .db
            .connection()?
            .get_sqlite_connection_pool()
            .is_closed()
    );
    fixture.reopen().await?;
    assert!(!fixture.table_exists("production_panic_probe").await?);
    assert_eq!(fixture.history_count().await?, 0);
    // 新标准pool上的生产迁移重取同一文件锁，不手工调用release替代panic路径。
    run_recovery(&fixture, Arc::new(ArmedGate::default())).await?;
    assert_eq!(fixture.history_count().await?, 1);
    fixture.close_production().await
}

#[tokio::test]
async fn production_cancel_finishes_current_native_migration_but_does_not_start_next() -> Result<()>
{
    let mut fixture = Fixture::new().await?;
    let gate = fixture.install_production_gate().await?;
    let cancellation = uncancelled();
    let work = fixture.production_work(gate.clone(), cancelling_migrations, cancellation.clone());
    let wrapper = tokio::spawn(super::sqlite_owner(
        None,
        tokio::time::Instant::now() + TEST_TIMEOUT,
        cancellation.clone(),
        Box::pin(work),
    ));
    entered(&fixture.gate).await?;
    cancellation.caller.cancel();
    fixture.assert_contended().await?;
    fixture.gate.release();
    let outcome = owner_result(wrapper).await?;
    assert_eq!(
        outcome.err().map(|error| error.code().to_owned()),
        Some("migration_cancelled".to_owned())
    );
    assert!(fixture.table_exists("production_current_probe").await?);
    assert!(!fixture.table_exists("production_next_probe").await?);
    assert_eq!(fixture.history_count().await?, 1);
    run_recovery(&fixture, gate).await?;
    assert_eq!(fixture.history_count().await?, 2);
    fixture.close_production().await
}

#[tokio::test]
async fn production_memory_schema_and_history_survive_each_owner_runtime_exit() -> Result<()> {
    static MEMORY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = MEMORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let named = format!(
        "sqlite:file:openrust-owner-lifecycle-{}-{sequence}?mode=memory&cache=shared",
        std::process::id(),
    );
    for (url, min_connections, max_connections) in
        [("sqlite::memory:", 0, 10), (named.as_str(), 1, 2)]
    {
        let config = DatabaseInstanceConfig {
            max_connections,
            min_connections,
            ..Default::default()
        };
        let connection = crate::db::connect_database(
            url,
            &config,
            DatabaseBackend::Sqlite,
            "migration memory lifecycle test",
            None,
            None,
        )
        .await?;
        let db = Db::default();
        db.initialize(connection)?;
        // 不建立额外anchor或借出物理连接；由生产调用链自行保持内存库与历史。
        let outcome: Result<()> = async {
            for pass in 1..=2 {
                let cancellation = uncancelled();
                let worker_cancellation = cancellation.clone();
                let worker_db = db.clone();
                let worker_url = url.to_owned();
                let work = async move {
                    PRODUCTION_GATE
                        .scope(Arc::new(ArmedGate::default()), async move {
                            super::perform(
                                &worker_db,
                                &worker_url,
                                &catalog(recovery_migrations),
                                super::super::MigrationAction::Up(None),
                                TEST_TIMEOUT,
                                &worker_cancellation,
                            )
                            .await
                        })
                        .await
                };
                // sqlite_owner在专用runtime析构后才发送结果；不手工drain/return_to_pool掩盖生产行为。
                super::sqlite_owner(
                    None,
                    tokio::time::Instant::now() + TEST_TIMEOUT,
                    cancellation,
                    Box::pin(work),
                )
                .await?;
                // 查询发生在原test runtime，不能依赖已退出owner的后台归还任务。
                let database = db.connection()?;
                assert!(
                    sea_orm_migration::SchemaManager::new(&database)
                        .has_table("production_recovery_probe")
                        .await
                        .map_err(Error::internal)?,
                    "owner runtime退出后第{pass}次内存迁移表丢失",
                );
                let row = database
                    .query_one_raw(sea_orm::Statement::from_string(
                        sea_orm::DbBackend::Sqlite,
                        "SELECT count(*) AS n FROM openrust_migrations",
                    ))
                    .await
                    .map_err(Error::internal)?
                    .ok_or_else(|| {
                        Error::custom("test_memory_history_missing", "内存迁移历史无查询结果")
                    })?;
                assert_eq!(
                    row.try_get::<i64>("", "n").map_err(Error::internal)?,
                    1,
                    "owner runtime退出后第{pass}次历史丢失或第二次up重复执行",
                );
            }
            Ok(())
        }
        .await;
        let closed = tokio::time::timeout(TEST_TIMEOUT, db.connection()?.close())
            .await
            .map_err(|_| Error::custom("test_memory_pool_close_timeout", "内存测试池关闭超时"))?
            .map_err(Error::internal);
        outcome?;
        closed?;
    }
    Ok(())
}

#[tokio::test]
async fn production_anchor_rejected_after_release_cannot_report_memory_migration_success()
-> Result<()> {
    let applied_seen = Arc::new(AtomicBool::new(false));
    let anchor_rejected = Arc::new(AtomicBool::new(false));
    let callback_seen = applied_seen.clone();
    let callback_rejected = anchor_rejected.clone();
    // 公共SQLx pool扩展故障注入：只按真实已持久化迁移历史决定是否拒绝归还，
    // 不绑定perform之前执行了多少次PRAGMA/建表/借连接。
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(0)
        .acquire_timeout(TEST_TIMEOUT)
        .after_release(move |connection, _| {
            let seen = callback_seen.clone();
            let rejected = callback_rejected.clone();
            Box::pin(async move {
                let exists: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'openrust_migrations'",
                )
                .fetch_one(&mut *connection)
                .await?;
                if exists == 0 {
                    return Ok(true);
                }
                let applied: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM openrust_migrations WHERE version = 'm20261102_000003_production_recovery'",
                )
                .fetch_one(&mut *connection)
                .await?;
                if applied != 1 {
                    return Ok(true);
                }
                if !seen.swap(true, Ordering::SeqCst) {
                    // history INSERT/commit后的首次归还允许完成。
                    return Ok(true);
                }
                // 此后生产anchor归还被拒绝：max1/min0意味着最后物理连接会被关闭。
                rejected.store(true, Ordering::SeqCst);
                Ok(false)
            })
        })
        .connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(
        pool.clone(),
    ))?;
    let cancellation = uncancelled();
    let worker_cancellation = cancellation.clone();
    let work = async move {
        PRODUCTION_GATE
            .scope(Arc::new(ArmedGate::default()), async move {
                super::perform(
                    &db,
                    "sqlite::memory:",
                    &catalog(recovery_migrations),
                    super::super::MigrationAction::Up(None),
                    TEST_TIMEOUT,
                    &worker_cancellation,
                )
                .await
            })
            .await
    };
    let outcome = super::sqlite_owner(
        None,
        tokio::time::Instant::now() + TEST_TIMEOUT,
        cancellation,
        Box::pin(work),
    )
    .await;
    // 在测试显式close前记录生产失败关闭事实，不能用测试清理伪造该断言。
    let closed_by_production = pool.is_closed();
    tokio::time::timeout(TEST_TIMEOUT, pool.close())
        .await
        .map_err(|_| {
            Error::custom(
                "test_rejected_anchor_pool_close_timeout",
                "故障注入池关闭超时",
            )
        })?;
    assert!(applied_seen.load(Ordering::SeqCst));
    assert!(anchor_rejected.load(Ordering::SeqCst));
    assert_eq!(
        outcome.err().map(|error| error.code().to_owned()),
        Some("migration_connection_return_failed".to_owned()),
        "after_release拒绝最后连接后，void return_to_pool不能被当作归还成功",
    );
    assert!(closed_by_production, "归还无法确认时生产路径必须关闭pool");
    Ok(())
}

struct ProceedReturnCheckOnDrop(Arc<super::residency::ReturnCheckGate>);

impl Drop for ProceedReturnCheckOnDrop {
    fn drop(&mut self) {
        self.0.proceed.notify_one();
    }
}

async fn borrowed_memory_snapshot(connection: &mut sqlx::SqliteConnection) -> Result<(i64, i64)> {
    let rows = sqlx::query_scalar("SELECT count(*) FROM production_recovery_probe")
        .fetch_one(&mut *connection)
        .await
        .map_err(Error::internal)?;
    let history = sqlx::query_scalar("SELECT count(*) FROM openrust_migrations")
        .fetch_one(&mut *connection)
        .await
        .map_err(Error::internal)?;
    Ok((rows, history))
}

struct ConcurrentReturnObservation {
    before: (i64, i64),
    after: (i64, i64),
    closed_by_production: bool,
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一回归保留owner、borrower和异常清理的完整生命周期"
)]
async fn production_return_check_accepts_same_memory_handle_checked_out_by_another_task()
-> Result<()> {
    static MEMORY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let named = format!(
        "sqlite:file:openrust-concurrent-return-{}-{}?mode=memory&cache=shared",
        std::process::id(),
        MEMORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    for url in ["sqlite::memory:", named.as_str()] {
        let config = DatabaseInstanceConfig {
            max_connections: 1,
            min_connections: 0,
            ..Default::default()
        };
        let connection = crate::db::connect_database(
            url,
            &config,
            DatabaseBackend::Sqlite,
            "concurrent memory return test",
            None,
            None,
        )
        .await?;
        let db = Db::default();
        db.initialize(connection)?;
        let pool = db.connection()?.get_sqlite_connection_pool().clone();
        let gate = Arc::new(super::residency::ReturnCheckGate::default());
        let _release_gate_on_failure = ProceedReturnCheckOnDrop(gate.clone());
        let owner_gate = gate.clone();
        let owner_db = db.clone();
        let owner_url = url.to_owned();
        let cancellation = uncancelled();
        let worker_cancellation = cancellation.clone();
        let work = async move {
            Box::pin(super::residency::with_return_check_gate(
                owner_gate,
                PRODUCTION_GATE.scope(Arc::new(ArmedGate::default()), async move {
                    super::perform(
                        &owner_db,
                        &owner_url,
                        &catalog(recovery_migrations),
                        super::super::MigrationAction::Up(None),
                        TEST_TIMEOUT,
                        &worker_cancellation,
                    )
                    .await
                }),
            ))
            .await
        };
        let mut owner = tokio::spawn(super::sqlite_owner(
            None,
            tokio::time::Instant::now() + TEST_TIMEOUT,
            cancellation,
            Box::pin(work),
        ));
        let (ready, ready_receiver) = oneshot::channel();
        let (recheck, recheck_receiver) = oneshot::channel::<()>();
        let (checked, checked_receiver) = oneshot::channel();
        let (release, release_receiver) = oneshot::channel::<()>();
        let mut borrower = None;
        let mut recheck = Some(recheck);
        let mut release = Some(release);
        let outcome: Result<ConcurrentReturnObservation> = async {
            tokio::time::timeout(TEST_TIMEOUT, gate.arrived.notified())
                .await
                .map_err(|_| {
                    Error::custom("test_return_check_gate_timeout", "生产连接未到达归还检查点")
                })?;
            let borrower_pool = pool.clone();
            borrower = Some(tokio::spawn(async move {
                // 生产anchor已经显式归还；此时才借出唯一物理连接，不是预置额外anchor。
                let mut connection = borrower_pool.acquire().await.map_err(Error::internal)?;
                let before = borrowed_memory_snapshot(&mut connection).await?;
                let _ = ready.send(before);
                let _ = recheck_receiver.await;
                let after = borrowed_memory_snapshot(&mut connection).await?;
                let _ = checked.send(after);
                let _ = release_receiver.await;
                connection.return_to_pool().await;
                Ok::<_, Error>(())
            }));
            let before = tokio::time::timeout(TEST_TIMEOUT, ready_receiver)
                .await
                .map_err(|_| Error::custom("test_memory_borrower_timeout", "并发借用未就绪"))?
                .map_err(|_| Error::custom("test_memory_borrower_lost", "并发借用查询失败"))?;
            gate.proceed.notify_one();
            // borrower仍持有唯一连接；生产不能用num_idle==0误报，也不能关闭业务池。
            tokio::time::timeout(TEST_TIMEOUT, &mut owner)
                .await
                .map_err(|_| Error::custom("test_return_owner_timeout", "归还检查后的owner未退出"))?
                .map_err(|_| Error::custom("test_return_owner_lost", "归还检查后的owner异常"))??;
            let closed = pool.is_closed();
            if let Some(recheck) = recheck.take() {
                let _ = recheck.send(());
            }
            let after = tokio::time::timeout(TEST_TIMEOUT, checked_receiver)
                .await
                .map_err(|_| {
                    Error::custom("test_memory_recheck_timeout", "owner退出后的借用查询未完成")
                })?
                .map_err(|_| {
                    Error::custom("test_memory_recheck_lost", "owner退出后的借用连接失效")
                })?;
            Ok(ConcurrentReturnObservation {
                before,
                after,
                closed_by_production: closed,
            })
        }
        .await;
        // 任何返回错误也先放行两端；不得让测试清理等待自己持有的唯一连接。
        gate.proceed.notify_one();
        if let Some(recheck) = recheck.take() {
            let _ = recheck.send(());
        }
        if let Some(release) = release.take() {
            let _ = release.send(());
        }
        let borrower_cleanup = if let Some(mut borrower) = borrower {
            if let Ok(result) = tokio::time::timeout(TEST_TIMEOUT, &mut borrower).await {
                result
                    .map_err(|_| Error::custom("test_memory_borrower_lost", "并发借用任务异常"))
                    .and_then(|result| result)
            } else {
                borrower.abort();
                let _ = borrower.await;
                Err(Error::custom(
                    "test_memory_borrower_timeout",
                    "并发借用清理超时",
                ))
            }
        } else {
            Ok(())
        };
        if !owner.is_finished()
            && tokio::time::timeout(TEST_TIMEOUT, &mut owner)
                .await
                .is_err()
        {
            owner.abort();
            let _ = owner.await;
        }
        let closed = tokio::time::timeout(TEST_TIMEOUT, pool.close())
            .await
            .map_err(|_| Error::custom("test_memory_pool_close_timeout", "并发归还测试池关闭超时"));
        let observation = outcome?;
        borrower_cleanup?;
        closed?;
        assert_eq!(observation.before, (0, 1));
        assert_eq!(observation.after, (0, 1));
        assert!(
            !observation.closed_by_production,
            "合法并发借出不能被误判为归还失败并关闭池"
        );
    }
    Ok(())
}
