//! 迁移文件锁的真实跨进程组件证据；生产 perform 绑定另由 execution/tests.rs 验证。

use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use super::SqliteMigrationLock;
use crate::{
    Error, Result,
    db::{DatabaseBackend, DatabaseInstanceConfig, Db},
};

const TIMEOUT: Duration = Duration::from_secs(20);
const CHILD_URL: &str = "OPENRUST_SQLITE_LOCK_CHILD_URL";
const CHILD_PHASE: &str = "OPENRUST_SQLITE_LOCK_CHILD_PHASE";
const SENTINEL: &str = "openrust_process_lock_sentinel";

async fn standard_database(url: &str) -> Result<Db> {
    let config = DatabaseInstanceConfig {
        max_connections: 1,
        min_connections: 1,
        ..Default::default()
    };
    let connection = crate::db::connect_database(
        url,
        &config,
        DatabaseBackend::Sqlite,
        "SQLite process lock test",
        None,
        None,
    )
    .await?;
    let db = Db::default();
    db.initialize(connection)?;
    Ok(db)
}

struct Fixture {
    directory: Option<tempfile::TempDir>,
    db: Db,
    url: String,
}

impl Fixture {
    async fn new() -> Result<Self> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join("database.sqlite");
        // 创建文件的 FD 在 SQLite 连接前关闭；测试不额外开闭活动主库文件。
        drop(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(Error::internal)?,
        );
        let uri = url::Url::from_file_path(&path)
            .map_err(|()| Error::custom("test_database_path_invalid", "测试数据库路径无效"))?;
        let url = uri.as_str().replacen("file:", "sqlite:", 1);
        let db = standard_database(&url).await?;
        Ok(Self {
            directory: Some(directory),
            db,
            url,
        })
    }

    async fn close(mut self) -> Result<()> {
        tokio::time::timeout(TIMEOUT, self.db.connection()?.close())
            .await
            .map_err(|_| Error::custom("test_database_close_timeout", "跨进程测试池关闭超时"))?
            .map_err(Error::internal)?;
        // 两个 child 都已 wait；父池关闭后才回收自有目录及其长期 sidecar。
        if let Some(directory) = self.directory.take() {
            directory.close().map_err(Error::internal)?;
        }
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let Some(directory) = self.directory.take() else {
            return;
        };
        let Ok(connection) = self.db.connection() else {
            std::mem::forget(directory);
            return;
        };
        // 错误路径仍先关闭池再删除；不依赖即将退出的测试runtime。
        let held = std::sync::Arc::new(std::sync::Mutex::new(Some(directory)));
        let worker_held = held.clone();
        let worker = std::thread::Builder::new()
            .name("sqlite-process-test-cleanup".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(runtime) = runtime {
                    let closed = runtime.block_on(async {
                        tokio::time::timeout(TIMEOUT, connection.close()).await
                    });
                    if matches!(closed, Ok(Ok(()))) {
                        return;
                    }
                }
                std::mem::forget(worker_held);
            });
        if worker.is_err() {
            std::mem::forget(held);
        }
    }
}

struct OwnedChild(Option<Child>);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn run_child(url: &str, phase: &str) -> Result<()> {
    // module_path带crate前缀，libtest的--exact名称不带crate前缀。
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, module)| module);
    let entry = format!("{module}::sqlite_lock_child_entry");
    let child = Command::new(std::env::current_exe().map_err(Error::internal)?)
        .args([
            "--ignored",
            "--exact",
            &entry,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_URL, url)
        .env(CHILD_PHASE, phase)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Error::internal)?;
    let mut child = OwnedChild(Some(child));
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let process = child
            .0
            .as_mut()
            .ok_or_else(|| Error::custom("test_child_missing", "测试子进程句柄丢失"))?;
        if process.try_wait().map_err(Error::internal)?.is_some() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            // OwnedChild析构强制kill+wait，不留下测试worker。
            return Err(Error::custom(
                "test_child_timeout",
                "跨进程互斥测试子进程超时",
            ));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let process = child
        .0
        .take()
        .ok_or_else(|| Error::custom("test_child_missing", "测试子进程句柄丢失"))?;
    let output = process.wait_with_output().map_err(Error::internal)?;
    let receipt = format!("OPENRUST_SQLITE_LOCK_RECEIPT:{phase}");
    if !output.status.success() || !String::from_utf8_lossy(&output.stderr).contains(&receipt) {
        eprintln!(
            "SQLite lock child phase={phase} status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(Error::custom(
            "test_child_failed",
            "跨进程测试子进程失败或缺少真实执行回执",
        ));
    }
    Ok(())
}

#[tokio::test]
async fn file_lock_excludes_other_process_then_allows_persistent_ddl() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let lock = SqliteMigrationLock::acquire(&fixture.db, &fixture.url, TIMEOUT).await?;
        run_child(&fixture.url, "contended").await?;
        let connection = fixture.db.connection()?;
        assert!(
            !sea_orm_migration::SchemaManager::new(&connection)
                .has_table(SENTINEL)
                .await
                .map_err(Error::internal)?
        );
        lock.release()?;
        run_child(&fixture.url, "acquired").await?;
        let row = connection
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT value FROM openrust_process_lock_sentinel",
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("test_child_ddl_missing", "子进程实际DDL数据未持久化"))?;
        assert_eq!(
            row.try_get::<i64>("", "value").map_err(Error::internal)?,
            73
        );
        Ok(())
    }
    .await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}

// ignored仅为current_exe子进程协议入口：上方非ignored父测试始终实际启动本入口两次。
// 不代表跳过跨进程能力验证，也不代表root完整Auth应用验收。
#[tokio::test]
#[ignore = "由跨进程父测试以唯一URL/phase显式启动；不是独立验收入口"]
async fn sqlite_lock_child_entry() -> Result<()> {
    let url = std::env::var(CHILD_URL).map_err(Error::internal)?;
    let phase = std::env::var(CHILD_PHASE).map_err(Error::internal)?;
    let db = standard_database(&url).await?;
    let outcome: Result<()> = async {
        match phase.as_str() {
            "contended" => {
                match SqliteMigrationLock::acquire(&db, &url, Duration::from_millis(200)).await {
                    Err(error) => assert_eq!(error.code(), "migration_lock_timeout"),
                    Ok(lock) => {
                        lock.release()?;
                        return Err(Error::custom(
                            "test_cross_process_lock_broken",
                            "父进程持锁时子进程意外取得锁",
                        ));
                    }
                }
                assert!(
                    !sea_orm_migration::SchemaManager::new(&db.connection()?)
                        .has_table(SENTINEL)
                        .await
                        .map_err(Error::internal)?
                );
            }
            "acquired" => {
                let lock = SqliteMigrationLock::acquire(&db, &url, Duration::from_secs(3)).await?;
                let connection = db.connection()?;
                let ddl: Result<()> = async {
                    connection
                        .execute_unprepared(
                            "CREATE TABLE openrust_process_lock_sentinel (value INTEGER NOT NULL)",
                        )
                        .await
                        .map_err(Error::internal)?;
                    connection
                        .execute_unprepared(
                            "INSERT INTO openrust_process_lock_sentinel (value) VALUES (73)",
                        )
                        .await
                        .map_err(Error::internal)?;
                    Ok(())
                }
                .await;
                let release = lock.release();
                ddl?;
                release?;
            }
            _ => {
                return Err(Error::custom(
                    "test_child_phase_invalid",
                    "跨进程测试phase无效",
                ));
            }
        }
        Ok(())
    }
    .await;
    let closed = tokio::time::timeout(TIMEOUT, db.connection()?.close())
        .await
        .map_err(|_| Error::custom("test_child_pool_close_timeout", "子进程数据库池关闭超时"))?
        .map_err(Error::internal);
    outcome?;
    closed?;
    eprintln!("OPENRUST_SQLITE_LOCK_RECEIPT:{phase}");
    Ok(())
}
