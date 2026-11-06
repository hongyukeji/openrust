//! 真实网络数据库的永久公开API子进程回归；仅操作本例明确拥有的资源。
#![cfg(all(unix, any(feature = "db-mysql", feature = "db-postgres")))]

use openrust::database::sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
};
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::testing::TestDatabase;
use openrust::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::unix::process::ExitStatusExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WAIT: Duration = Duration::from_secs(90);
const MODE: &str = "OPENRUST_LIFECYCLE_CHILD_MODE";
const RECEIPT: &str = "OPENRUST_LIFECYCLE_CHILD_RECEIPT";
const STATE: &str = "OPENRUST_TEST_DATABASE_STATE_DIR";
const MARKER: &str = "__openrust_test_database_owner";

fn failure(code: &'static str) -> Error {
    Error::custom(code, "数据库生命周期回归未满足当前步骤；连接信息已隐藏")
}
fn check(value: bool, code: &'static str) -> Result<()> {
    if value { Ok(()) } else { Err(failure(code)) }
}
struct EmptyMigrator;
#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[derive(Serialize, Deserialize)]
struct Ready {
    name: String,
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| failure("lifecycle_receipt_encode"))?;
    let pending = path.with_extension("pending");
    let mut file = fs::File::create(&pending).map_err(|_| failure("lifecycle_receipt_create"))?;
    std::io::Write::write_all(&mut file, &bytes).map_err(|_| failure("lifecycle_receipt_write"))?;
    file.sync_all()
        .map_err(|_| failure("lifecycle_receipt_sync"))?;
    fs::rename(pending, path).map_err(|_| failure("lifecycle_receipt_publish"))
}
fn name_from_url(value: &str) -> Result<String> {
    let parsed = url::Url::parse(value).map_err(|_| failure("lifecycle_url_parse"))?;
    let name = parsed.path().trim_start_matches('/').to_owned();
    validate_name(&name)?;
    Ok(name)
}
fn validate_name(name: &str) -> Result<()> {
    check(
        name.starts_with("openrust_")
            && name.len() <= 63
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "lifecycle_owned_name_invalid",
    )
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| failure("lifecycle_runtime_create"))
}

/// 子进程协议入口；仅父用例传入mode时执行，不能当独立服务验收运行。
#[test]
#[ignore = "子进程专用；通过lifecycle_recovery_public_api入口调度"]
fn lifecycle_child_entry() -> Result<()> {
    let mode = std::env::var(MODE).map_err(|_| failure("lifecycle_child_mode_missing"))?;
    let admin = std::env::var("OPENRUST_TEST_DATABASE_URL")
        .map_err(|_| failure("lifecycle_database_url_missing"))?;
    let receipt = PathBuf::from(
        std::env::var_os(RECEIPT).ok_or_else(|| failure("lifecycle_receipt_path_missing"))?,
    );
    let runtime = runtime()?;
    if mode == "recover" {
        let recovered = runtime
            .block_on(TestDatabase::recover(&admin))
            .map_err(|_| failure("lifecycle_recover_failed"))?;
        return write_json(
            &receipt,
            &json!({"reclaimed":recovered.reclaimed,"active":recovered.active,"retained":recovered.retained}),
        );
    }
    let database = runtime
        .block_on(TestDatabase::create::<EmptyMigrator>(&admin))
        .map_err(|_| failure("lifecycle_create_failed"))?;
    write_json(
        &receipt,
        &Ready {
            name: name_from_url(database.url())?,
        },
    )?;
    match mode.as_str() {
        "close" => runtime
            .block_on(database.close())
            .map_err(|_| failure("lifecycle_close_failed")),
        "drop" => {
            runtime.block_on(async {
                drop(database);
                tokio::task::yield_now().await;
            });
            Ok(())
        }
        "runtime-exit" => {
            drop(runtime);
            drop(database);
            Ok(())
        }
        "panic" => std::panic::resume_unwind(Box::new("intentional owned lifecycle unwind")),
        "hold" => {
            while !receipt.with_extension("release").exists() {
                std::thread::sleep(Duration::from_millis(20));
            }
            runtime
                .block_on(database.close())
                .map_err(|_| failure("lifecycle_hold_close_failed"))
        }
        _ => Err(failure("lifecycle_child_mode_invalid")),
    }
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn spawn(admin: &str, root: &Path, receipt: &Path, mode: &str) -> Result<Worker> {
    let mut command =
        Command::new(std::env::current_exe().map_err(|_| failure("lifecycle_executable_missing"))?);
    command
        .args([
            "--ignored",
            "--exact",
            "lifecycle_child_entry",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("OPENRUST_TEST_DATABASE_URL", admin)
        .env(STATE, root)
        .env(MODE, mode)
        .env(RECEIPT, receipt)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(Worker)
        .map_err(|_| failure("lifecycle_worker_spawn_failed"))
}
fn wait(worker: &mut Worker) -> Result<ExitStatus> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = worker
            .0
            .try_wait()
            .map_err(|_| failure("lifecycle_worker_poll_failed"))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(failure("lifecycle_worker_deadline"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn ready(worker: &mut Worker, path: &Path) -> Result<Ready> {
    let deadline = Instant::now() + WAIT;
    loop {
        if path.exists() {
            let record: Ready = serde_json::from_slice(
                &fs::read(path).map_err(|_| failure("lifecycle_receipt_read"))?,
            )
            .map_err(|_| failure("lifecycle_receipt_invalid"))?;
            validate_name(&record.name)?;
            return Ok(record);
        }
        check(
            worker
                .0
                .try_wait()
                .map_err(|_| failure("lifecycle_worker_poll_failed"))?
                .is_none(),
            "lifecycle_worker_left_before_ready",
        )?;
        check(Instant::now() < deadline, "lifecycle_ready_deadline")?;
        std::thread::sleep(Duration::from_millis(20));
    }
}

// 未确认清理时保留独占journal目录，不能在失败析构时丢掉恢复责任证据。
struct Scenario {
    directory: Option<tempfile::TempDir>,
    root: PathBuf,
    receipt: PathBuf,
    sequence: usize,
}
impl Scenario {
    fn new() -> Result<Self> {
        let directory = tempfile::tempdir().map_err(|_| failure("lifecycle_tempdir_failed"))?;
        let root = directory.path().join("journal");
        let receipt = directory.path().join("ready.json");
        Ok(Self {
            directory: Some(directory),
            root,
            receipt,
            sequence: 0,
        })
    }
    fn recover(&mut self, admin: &str) -> Result<Value> {
        self.sequence += 1;
        let receipt = self
            .receipt
            .with_file_name(format!("recovery-{}.json", self.sequence));
        let mut worker = spawn(admin, &self.root, &receipt, "recover")?;
        check(
            wait(&mut worker)?.success(),
            "lifecycle_recovery_process_failed",
        )?;
        serde_json::from_slice(
            &fs::read(receipt).map_err(|_| failure("lifecycle_recovery_receipt_read"))?,
        )
        .map_err(|_| failure("lifecycle_recovery_receipt_invalid"))
    }
    fn completed(&mut self) {
        drop(self.directory.take());
    }
}
impl Drop for Scenario {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            let preserved = directory.keep();
            eprintln!("保留本用例数据库恢复证据：{}", preserved.display());
        }
    }
}

async fn connect(url: &str) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_owned());
    options
        .max_connections(2)
        .min_connections(0)
        .connect_timeout(Duration::from_secs(5))
        .acquire_timeout(Duration::from_secs(5));
    tokio::time::timeout(Duration::from_secs(10), Database::connect(options))
        .await
        .map_err(|_| failure("lifecycle_connect_timeout"))?
        .map_err(|_| failure("lifecycle_connect_failed"))
}
async fn exists(admin: &DatabaseConnection, name: &str) -> Result<bool> {
    validate_name(name)?;
    let sql = match admin.get_database_backend() {
        DbBackend::MySql => {
            "SELECT CAST(COUNT(*) AS SIGNED) AS present FROM information_schema.schemata WHERE SCHEMA_NAME=?"
        }
        DbBackend::Postgres => {
            "SELECT COUNT(*)::bigint AS present FROM pg_database WHERE datname=$1"
        }
        _ => return Err(failure("lifecycle_backend_not_network")),
    };
    let row = admin
        .query_one_raw(Statement::from_sql_and_values(
            admin.get_database_backend(),
            sql,
            [name.into()],
        ))
        .await
        .map_err(|_| failure("lifecycle_catalog_query_failed"))?
        .ok_or_else(|| failure("lifecycle_catalog_row_missing"))?;
    Ok(row
        .try_get::<i64>("", "present")
        .map_err(|_| failure("lifecycle_catalog_decode_failed"))?
        == 1)
}
async fn execute(admin: &DatabaseConnection, sql: String) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), admin.execute_unprepared(&sql))
        .await
        .map_err(|_| failure("lifecycle_sql_timeout"))?
        .map_err(|_| failure("lifecycle_sql_failed"))?;
    Ok(())
}
fn quote(admin: &DatabaseConnection, name: &str) -> Result<String> {
    validate_name(name)?;
    Ok(if admin.get_database_backend() == DbBackend::MySql {
        format!("`{name}`")
    } else {
        format!("\"{name}\"")
    })
}

fn kill_owned(worker: &mut Worker) -> Result<()> {
    let status = Command::new("kill")
        .arg("-KILL")
        .arg(worker.0.id().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| failure("lifecycle_kill_command_failed"))?;
    check(status.success(), "lifecycle_kill_failed")?;
    check(wait(worker)?.signal() == Some(9), "lifecycle_not_sigkill")
}

async fn normal_case(admin_url: &str, admin: &DatabaseConnection, mode: &str) -> Result<()> {
    let mut scenario = Scenario::new()?;
    let mut worker = spawn(admin_url, &scenario.root, &scenario.receipt, mode)?;
    let owned = ready(&mut worker, &scenario.receipt)?;
    let status = wait(&mut worker)?;
    let outcome: Result<()> = async {
        check(
            status.success() != (mode == "panic"),
            "lifecycle_wrong_normal_status",
        )?;
        check(
            !exists(admin, &owned.name).await?,
            "lifecycle_normal_exit_left_database",
        )
    }
    .await;
    drop(worker);
    let recovered = scenario.recover(admin_url);
    outcome?;
    let recovered = recovered?;
    check(
        recovered["retained"] == 0 && recovered["active"] == 0,
        "lifecycle_normal_journal_not_final",
    )?;
    scenario.completed();
    Ok(())
}

async fn crash_case(admin_url: &str, admin: &DatabaseConnection) -> Result<()> {
    let mut scenario = Scenario::new()?;
    let mut worker = spawn(admin_url, &scenario.root, &scenario.receipt, "hold")?;
    let owned = ready(&mut worker, &scenario.receipt)?;
    kill_owned(&mut worker)?;
    check(
        exists(admin, &owned.name).await?,
        "lifecycle_sigkill_ran_cleanup",
    )?;
    let recovered = scenario.recover(admin_url)?;
    check(
        recovered["reclaimed"]
            .as_u64()
            .is_some_and(|value| value >= 1),
        "lifecycle_sigkill_not_reclaimed",
    )?;
    check(
        recovered["retained"] == 0 && recovered["active"] == 0,
        "lifecycle_sigkill_recovery_not_final",
    )?;
    check(
        !exists(admin, &owned.name).await?,
        "lifecycle_sigkill_database_retained",
    )?;
    scenario.completed();
    Ok(())
}

async fn already_missing_case(admin_url: &str, admin: &DatabaseConnection) -> Result<()> {
    let mut scenario = Scenario::new()?;
    let mut worker = spawn(admin_url, &scenario.root, &scenario.receipt, "hold")?;
    let owned = ready(&mut worker, &scenario.receipt)?;
    kill_owned(&mut worker)?;
    // 仅删除本例CREATE成功并有精确receipt的库，以验证未知DROP后缺库的终态补偿。
    let target = quote(admin, &owned.name)?;
    execute(admin, format!("DROP DATABASE {target}")).await?;
    let recovered = scenario.recover(admin_url)?;
    check(
        recovered["reclaimed"]
            .as_u64()
            .is_some_and(|count| count >= 1)
            && recovered["retained"] == 0
            && recovered["active"] == 0,
        "lifecycle_absent_database_not_final",
    )?;
    scenario.completed();
    Ok(())
}

async fn active_case(admin_url: &str, admin: &DatabaseConnection) -> Result<()> {
    let mut scenario = Scenario::new()?;
    let mut worker = spawn(admin_url, &scenario.root, &scenario.receipt, "hold")?;
    let owned = ready(&mut worker, &scenario.receipt)?;
    let outcome: Result<()> = async {
        let recovered = scenario.recover(admin_url)?;
        check(
            recovered["active"].as_u64().is_some_and(|value| value >= 1),
            "lifecycle_live_guard_not_active",
        )?;
        check(
            recovered["reclaimed"] == 0 && exists(admin, &owned.name).await?,
            "lifecycle_live_guard_was_reclaimed",
        )
    }
    .await;
    fs::write(scenario.receipt.with_extension("release"), [])
        .map_err(|_| failure("lifecycle_release_failed"))?;
    let stopped = wait(&mut worker);
    drop(worker);
    let recovered = scenario.recover(admin_url);
    outcome?;
    check(stopped?.success(), "lifecycle_live_close_failed")?;
    let recovered = recovered?;
    check(
        recovered["retained"] == 0 && recovered["active"] == 0,
        "lifecycle_live_cleanup_not_final",
    )?;
    check(
        !exists(admin, &owned.name).await?,
        "lifecycle_live_database_retained",
    )?;
    scenario.completed();
    Ok(())
}

async fn marker_comment(admin: &DatabaseConnection, name: &str) -> Result<String> {
    let sql = if admin.get_database_backend() == DbBackend::MySql {
        "SELECT ROUTINE_COMMENT AS marker FROM information_schema.routines WHERE ROUTINE_SCHEMA=? AND ROUTINE_NAME='__openrust_test_database_owner' AND ROUTINE_TYPE='PROCEDURE'"
    } else {
        "SELECT shobj_description(oid, 'pg_database') AS marker FROM pg_database WHERE datname=$1"
    };
    let row = admin
        .query_one_raw(Statement::from_sql_and_values(
            admin.get_database_backend(),
            sql,
            [name.into()],
        ))
        .await
        .map_err(|_| failure("lifecycle_marker_read_failed"))?
        .ok_or_else(|| failure("lifecycle_marker_missing"))?;
    row.try_get::<Option<String>>("", "marker")
        .map_err(|_| failure("lifecycle_marker_decode_failed"))?
        .ok_or_else(|| failure("lifecycle_marker_empty"))
}
async fn set_marker(admin: &DatabaseConnection, name: &str, marker: &str) -> Result<()> {
    let target = quote(admin, name)?;
    let marker = marker.replace('\'', "''");
    let sql = if admin.get_database_backend() == DbBackend::MySql {
        format!("ALTER PROCEDURE {target}.`{MARKER}` COMMENT '{marker}'")
    } else {
        format!("COMMENT ON DATABASE {target} IS '{marker}'")
    };
    execute(admin, sql).await
}
async fn marker_case(admin_url: &str, admin: &DatabaseConnection) -> Result<()> {
    let mut scenario = Scenario::new()?;
    let mut worker = spawn(admin_url, &scenario.root, &scenario.receipt, "hold")?;
    let owned = ready(&mut worker, &scenario.receipt)?;
    kill_owned(&mut worker)?;
    let original = marker_comment(admin, &owned.name).await?;
    let outcome: Result<()> = async {
        set_marker(
            admin,
            &owned.name,
            "deliberately-mismatched-owned-test-marker",
        )
        .await?;
        let recovered = scenario.recover(admin_url)?;
        check(
            recovered["retained"]
                .as_u64()
                .is_some_and(|value| value >= 1),
            "lifecycle_mismatched_marker_not_retained",
        )?;
        check(
            recovered["reclaimed"] == 0 && exists(admin, &owned.name).await?,
            "lifecycle_mismatched_marker_database_deleted",
        )
    }
    .await;
    let restored = set_marker(admin, &owned.name, &original).await;
    let recovered = if restored.is_ok() {
        scenario.recover(admin_url)
    } else {
        Err(failure("lifecycle_marker_restore_failed"))
    };
    outcome?;
    restored?;
    let recovered = recovered?;
    check(
        recovered["retained"] == 0 && !exists(admin, &owned.name).await?,
        "lifecycle_marker_recovery_not_final",
    )?;
    scenario.completed();
    Ok(())
}

async fn prefix_case(admin_url: &str, admin: &DatabaseConnection) -> Result<()> {
    let mut scenario = Scenario::new()?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| failure("lifecycle_clock_invalid"))?
        .as_nanos();
    let name = format!("openrust_unclaimed_{}_{time}", std::process::id());
    check(
        !exists(admin, &name).await?,
        "lifecycle_fake_name_collision",
    )?;
    let target = quote(admin, &name)?;
    execute(admin, format!("CREATE DATABASE {target}")).await?;
    let outcome: Result<()> = async {
        let recovered = scenario.recover(admin_url)?;
        check(
            recovered["reclaimed"] == 0 && exists(admin, &name).await?,
            "lifecycle_prefix_scan_deleted_unclaimed",
        )
    }
    .await;
    // CREATE成功且该精确名字在本例产生，才有权显式回收；不按前缀枚举或删库。
    let cleanup = execute(admin, format!("DROP DATABASE {target}")).await;
    outcome?;
    cleanup?;
    scenario.completed();
    Ok(())
}

async fn application_pool_case(admin_url: &str, admin: &DatabaseConnection) -> Result<()> {
    let mut scenario = Scenario::new()?;
    let mut worker = spawn(admin_url, &scenario.root, &scenario.receipt, "hold")?;
    let owned = ready(&mut worker, &scenario.receipt)?;
    let mut application_url =
        url::Url::parse(admin_url).map_err(|_| failure("lifecycle_url_parse"))?;
    application_url.set_path(&format!("/{}", owned.name));
    let application = connect(application_url.as_str()).await?;
    application
        .execute_unprepared("SELECT 1")
        .await
        .map_err(|_| failure("lifecycle_application_query_failed"))?;
    kill_owned(&mut worker)?;
    let outcome: Result<()> = async {
        let recovered = scenario.recover(admin_url)?;
        check(
            recovered["retained"]
                .as_u64()
                .is_some_and(|value| value >= 1),
            "lifecycle_application_pool_not_retained",
        )?;
        check(
            recovered["reclaimed"] == 0 && exists(admin, &owned.name).await?,
            "lifecycle_application_pool_database_deleted",
        )?;
        application
            .execute_unprepared("SELECT 1")
            .await
            .map_err(|_| failure("lifecycle_application_pool_was_terminated"))?;
        Ok(())
    }
    .await;
    let closed = application
        .close()
        .await
        .map_err(|_| failure("lifecycle_application_close_failed"));
    let recovered = scenario.recover(admin_url);
    outcome?;
    closed?;
    let recovered = recovered?;
    check(
        recovered["retained"] == 0 && !exists(admin, &owned.name).await?,
        "lifecycle_application_pool_recovery_not_final",
    )?;
    scenario.completed();
    Ok(())
}

/// 真实服务入口：必须显式启用并提供有建库权限的现有测试URL；不以缺URL跳过冒充执行。
#[test]
#[ignore = "真实MySQL/PostgreSQL：设置OPENRUST_TEST_DATABASE_LIFECYCLE=1及OPENRUST_TEST_DATABASE_URL，再--ignored --exact lifecycle_recovery_public_api"]
fn lifecycle_recovery_public_api() -> Result<()> {
    check(
        std::env::var("OPENRUST_TEST_DATABASE_LIFECYCLE").as_deref() == Ok("1"),
        "lifecycle_explicit_service_gate_missing",
    )?;
    let admin_url = std::env::var("OPENRUST_TEST_DATABASE_URL")
        .map_err(|_| failure("lifecycle_database_url_missing"))?;
    let runtime = runtime()?;
    runtime.block_on(async {
        let admin = connect(&admin_url).await?;
        check(
            matches!(
                admin.get_database_backend(),
                DbBackend::MySql | DbBackend::Postgres
            ),
            "lifecycle_backend_not_network",
        )?;
        let outcome: Result<()> = async {
            for mode in ["close", "drop", "runtime-exit", "panic"] {
                normal_case(&admin_url, &admin, mode).await?;
                eprintln!("verified {mode}");
            }
            crash_case(&admin_url, &admin).await?;
            eprintln!("verified sigkill-recover");
            already_missing_case(&admin_url, &admin).await?;
            eprintln!("verified absent-database-finalized");
            active_case(&admin_url, &admin).await?;
            eprintln!("verified active-owner-retained");
            marker_case(&admin_url, &admin).await?;
            eprintln!("verified marker-mismatch-retained");
            prefix_case(&admin_url, &admin).await?;
            eprintln!("verified fake-prefix-retained");
            application_pool_case(&admin_url, &admin).await?;
            eprintln!("verified external-pool-retained-and-recovered");
            Ok(())
        }
        .await;
        let closed = admin
            .close()
            .await
            .map_err(|_| failure("lifecycle_admin_close_failed"));
        outcome?;
        closed
    })
}
