//! 为 PostgreSQL、MySQL 和 SQLite 的每个测试创建独立数据库。

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    Error, Result,
    database::{DatabaseBackend, DatabaseTableNames, sea_orm_migration::MigratorTrait},
};
use futures_util::FutureExt as _;
use sea_orm::DatabaseConnection;
#[cfg(test)]
use sea_orm::{ConnectOptions, Database};
use url::Url;

static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

mod exit;
mod lease;
mod marker;
mod owned_pool;
mod ownership;
mod resources;
mod session;
use ownership::ResourceKind;
use resources::OwnedDatabase;
pub use resources::TestDatabaseRecovery;

/// 为单个集成测试隔离的数据库。
///
/// 使用 [`TestDatabase::create`] 创建，并将 [`url`](Self::url) 用作应用的 `[database].url`。
/// 调用 [`close`](Self::close) 等待清理完成。网络库的清理由短寿命线程自驱，
/// 不依赖调用方 Tokio runtime 继续存活。异步析构只移交清理以允许还池任务推进；
/// 正常进程退出前有界等待已派发的操作，强杀与 abort 不运行退出回调。
pub struct TestDatabase {
    database_name: String,
    database_url: String,
    connection: Option<DatabaseConnection>,
    cleaned: bool,
    sqlite_runtime: Option<Box<crate::foundation::Application>>,
    sqlite_directory: Option<std::path::PathBuf>,
    sqlite: bool,
    // 显式关闭开始后，失败/取消必须保留文件，不能再由 Drop 无条件重试删除。
    preserve_sqlite: bool,
    // 网络池的 I/O 驱动随 guard 交接；不能依赖正被 Drop 等待阻塞的调用方 runtime。
    network_runtime: Option<tokio::runtime::Runtime>,
    resources: Vec<OwnedDatabase>,
    #[cfg(test)]
    force_cleanup_start_failure: Option<()>,
}

impl TestDatabase {
    /// 创建由本 guard 持有的唯一共享内存 SQLite 数据库，并执行迁移。
    ///
    /// 被测应用可独立连接 [`Self::url`]；guard 关闭前共享内存库一直存在。
    pub async fn sqlite_memory<M>() -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        Self::create_sqlite::<M>(false, DatabaseTableNames::default()).await
    }

    /// 创建自有临时目录中的 SQLite 文件，并在关闭连接后回收文件及 WAL/SHM。
    ///
    /// 此方法不接收已有路径，因此不会删除调用方的数据库。
    pub async fn sqlite_file<M>() -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        Self::create_sqlite::<M>(true, DatabaseTableNames::default()).await
    }

    async fn create_sqlite<M>(file: bool, table_names: DatabaseTableNames) -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        if !cfg!(feature = "db-sqlite") {
            return Err(Error::custom(
                "database_feature_disabled",
                "enable the testing db-sqlite feature",
            ));
        }
        let database_name = unique_database_name();
        let mut directory = None;
        let database_url = if file {
            let path = std::env::temp_dir().join(&database_name);
            let database = path.join("database.sqlite");
            let uri = Url::from_file_path(&database).map_err(|()| {
                Error::custom("test_database_path_invalid", "临时数据库路径无法转换为 URL")
            })?;
            std::fs::create_dir(&path).map_err(Error::internal)?;
            if let Err(error) = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&database)
            {
                let _ = std::fs::remove_dir(&path);
                return Err(Error::internal(error));
            }

            // url crate 的文件编码保留空格等路径字符，连接器负责解析 sqlite scheme。
            let sqlite_url = uri.as_str().replacen("file:", "sqlite:", 1);
            directory = Some(path);
            sqlite_url
        } else {
            format!("sqlite://file:{database_name}?mode=memory&cache=shared")
        };
        let mut guard = Self {
            database_name,
            database_url,
            connection: None,
            cleaned: false,
            sqlite_runtime: None,
            sqlite_directory: directory,
            sqlite: true,
            preserve_sqlite: false,
            network_runtime: None,
            resources: Vec::new(),
            #[cfg(test)]
            force_cleanup_start_failure: None,
        };
        let setup = async {
            let mut builder = crate::foundation::ProviderBuilder::new();
            builder.provider(crate::database::DatabaseServiceProvider);
            builder.provider(crate::database::MigrationServiceProvider::<M>::new());
            let runtime = builder
                .start(figment::Figment::from(
                    figment::providers::Serialized::defaults(serde_json::json!({"database": {
                        "url": guard.database_url, "max_connections": 2, "min_connections": 0,
                        "table_prefix": table_names.prefix(), "table_names": table_names.mappings(),
                    }})),
                ))
                .await?;
            // 标准App命令拥有SQLite迁移执行/取消边界；不能手工伪造执行所有者。
            guard.sqlite_runtime = Some(Box::new(
                crate::foundation::Application::from_provider_runtime(runtime)?,
            ));
            let app = guard.sqlite_runtime.as_ref().ok_or_else(|| {
                Error::custom(
                    "test_database_runtime_missing",
                    "测试数据库缺少受管生命周期",
                )
            })?;
            let db = app.state().resolve::<crate::database::Db>().await?;
            guard.connection = Some(db.connection()?);
            app.run_command("migrate", Vec::new()).await
        }
        .await;
        if let Err(error) = setup {
            let _ = guard.close().await;
            return Err(error);
        }
        Ok(guard)
    }

    /// 创建唯一命名的数据库，并应用应用程序的迁移。
    ///
    /// URL 必须直连唯一可写主库的既有管理数据库，当前用户须有建库、删库与归属核验权限。
    /// MySQL 额外要求 CREATE ROUTINE 和完整 `performance_schema` 连接/状态可见性；
    /// PostgreSQL 要求读取 `pg_control_system()` 与连接活动。拒绝只读副本。
    /// PostgreSQL 使用临时迁移模板；MySQL 则在隔离数据库中重放迁移。
    /// 仅清理本次 CREATE 成功取得所有权的库；同名创建失败不会删除已有库。
    /// SQLite 内存 URL 会转换为新的自有共享内存库；文件 URL 必须改用 [`Self::sqlite_file`]。
    pub async fn create<M>(admin_url: &str) -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        Self::create_with_table_names::<M>(admin_url, DatabaseTableNames::default()).await
    }

    /// 创建隔离数据库，并在指定的表名作用域中运行迁移。
    pub async fn create_with_table_names<M>(
        admin_url: &str,
        table_names: DatabaseTableNames,
    ) -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        Self::create_named_with_table_names::<M>(admin_url, table_names, unique_database_name())
            .await
    }

    async fn create_named_with_table_names<M>(
        admin_url: &str,
        table_names: DatabaseTableNames,
        database_name: String,
    ) -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        let backend = DatabaseBackend::from_url(admin_url)?;
        if backend == DatabaseBackend::Sqlite {
            if sqlite_memory_url(admin_url) {
                return Box::pin(Self::create_sqlite::<M>(false, table_names)).await;
            }
            return Err(owned_file_source_error());
        }
        require_backend_feature(backend)?;
        let admin_url = admin_url.to_owned();
        let exit_ticket = exit::register_create_operation()?;
        let (mut sender, result) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("openrust-test-db-create".to_owned())
            .spawn(move || {
                let _exit = exit_ticket;
                let Ok(runtime) = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build() else {
                    let _ = sender.send(Err(cleanup_error("test_database_create_runtime_failed")));
                    return;
                };
                let mut outcome = runtime.block_on(async {
                    let mut created = CreatedDatabases::default();
                    let outcome = {
                        let prepare = std::panic::AssertUnwindSafe(Self::prepare_network_database::<M>(
                            &admin_url, table_names, database_name.clone(), &mut created,
                        )).catch_unwind();
                        tokio::select! {
                            value = prepare => value.unwrap_or_else(|_| Err(cleanup_error("test_database_setup_panicked"))),
                            () = sender.closed() => Err(cleanup_error("test_database_create_cancelled")),
                        }
                    };
                    if outcome.is_err() {
                        cleanup_created_databases(&mut created).await;
                    }
                    outcome
                });
                match &mut outcome {
                    Ok(database) => database.network_runtime = Some(runtime),
                    Err(_) => runtime.shutdown_timeout(std::time::Duration::from_secs(1)),
                }
                let _ = sender.send(outcome);
            })
            .map_err(|_| cleanup_error("test_database_create_worker_failed"))?;
        let operation = NetworkOperation {
            result: Some(result),
        };
        operation.receive().await
    }

    async fn prepare_network_database<M>(
        admin_url: &str,
        table_names: DatabaseTableNames,
        database_name: String,
        created: &mut CreatedDatabases,
    ) -> Result<Self>
    where
        M: MigratorTrait + Send + Sync + 'static,
    {
        let backend = DatabaseBackend::from_url(admin_url)?;
        let parsed = Url::parse(admin_url).map_err(Error::internal)?;
        let database_url = url_for_database(&parsed, &database_name);
        let root = resources::state_root()?;
        let recovery = resources::recover(admin_url, &root).await?;
        if recovery.retained > 0 {
            tracing::warn!(
                retained = recovery.retained,
                "有测试资源未通过归属核验，已保留"
            );
        }
        created.resources.push(
            OwnedDatabase::plan(admin_url, &root, &database_name, ResourceKind::Target).await?,
        );
        match backend {
            DatabaseBackend::MySql => {
                created.resources[0].create(None).await?;
                created.resources[0].migration_pool = Some(
                    owned_pool::connect(&database_url, created.resources[0].journal.record())
                        .await?,
                );
                created.resources[0].migration_started = true;
                let pool = created.resources[0]
                    .migration_pool
                    .as_ref()
                    .ok_or_else(|| cleanup_error("test_database_pool_missing"))?;
                table_names
                    .scope(M::up(pool, None))
                    .await
                    .map_err(Error::internal)?;
                created.resources[0].close_migration_pool().await?;
                created.resources[0].check_marker().await?;
            }
            DatabaseBackend::PostgreSql => {
                let template_name = format!("{database_name}_template");
                created.resources.push(
                    OwnedDatabase::plan(admin_url, &root, &template_name, ResourceKind::Template)
                        .await?,
                );
                created.resources[1].create(None).await?;
                let template_url = url_for_database(&parsed, &template_name);
                created.resources[1].migration_pool = Some(
                    owned_pool::connect(&template_url, created.resources[1].journal.record())
                        .await?,
                );
                created.resources[1].migration_started = true;
                let pool = created.resources[1]
                    .migration_pool
                    .as_ref()
                    .ok_or_else(|| cleanup_error("test_database_pool_missing"))?;
                table_names
                    .scope(M::up(pool, None))
                    .await
                    .map_err(Error::internal)?;
                created.resources[1].close_migration_pool().await?;
                created.resources[1].check_marker().await?;
                created.resources[1]
                    .lease()?
                    .require_idle(&template_name)
                    .await?;
                created.resources[0].create(Some(&template_name)).await?;
                created.resources[0].migration_started = true;
                created.resources[0].check_marker().await?;
                created.resources[1].cleanup().await?;
                created.resources[1].close_lease().await?;
                drop(created.resources.pop());
            }
            DatabaseBackend::Sqlite => return Err(owned_file_source_error()),
        }
        let connection =
            owned_pool::connect(&database_url, created.resources[0].journal.record()).await?;
        Ok(Self {
            database_name,
            database_url,
            connection: Some(connection),
            cleaned: false,
            sqlite_runtime: None,
            sqlite_directory: None,
            sqlite: false,
            preserve_sqlite: false,
            network_runtime: None,
            resources: std::mem::take(&mut created.resources),
            #[cfg(test)]
            force_cleanup_start_failure: None,
        })
    }

    /// 从本机私有回执补偿已失活的网络测试库；缺失或错配的见证只保留，不接管旧前缀库。
    ///
    /// SQLite不参与此恢复；本方法不强制终止任何数据库连接。
    /// 回执位于 storage/framework/testing/databases，或进程环境变量
    /// `OPENRUST_TEST_DATABASE_STATE_DIR` 指定的稳定私有目录；不可删除活跃锁文件。
    /// 网络恢复当前要求 Unix 私有权限，非 Unix 明确拒绝。
    pub async fn recover(admin_url: &str) -> Result<TestDatabaseRecovery> {
        let backend = DatabaseBackend::from_url(admin_url)?;
        if backend == DatabaseBackend::Sqlite {
            return Ok(TestDatabaseRecovery::default());
        }
        require_backend_feature(backend)?;
        resources::recover(admin_url, &resources::state_root()?).await
    }

    /// 返回用于配置被测应用的连接 URL。
    pub fn url(&self) -> &str {
        &self.database_url
    }

    /// 返回隔离 SeaORM 连接池的克隆。
    pub fn connection(&self) -> Result<DatabaseConnection> {
        self.connection
            .clone()
            .ok_or_else(|| Error::custom("test_database_closed", "test database is already closed"))
    }

    /// 关闭连接池并删除隔离数据库。
    ///
    /// 调用前须先关闭借用该 URL 的独立应用/worker 池，并停止创建新借用。
    /// 不强制终止外部连接；网络库占用或归属不明时保留回执供下次恢复。
    /// 调用时即禁止 `Drop` 另行删除；关闭 future 失败、取消或未被轮询时均保留目录。
    pub fn close(mut self) -> crate::support::BoxFuture<'static, Result<()>> {
        if !self.sqlite {
            let cleanup = self.start_network_cleanup();
            return Box::pin(async move { cleanup?.receive().await });
        }
        if self.sqlite {
            self.preserve_sqlite = true;
        }
        Box::pin(self.close_sqlite_inner())
    }

    // 调用即移交唯一删除责任；close future 未 poll 或取消不能撤销已开始的回收。
    fn start_network_cleanup(&mut self) -> Result<NetworkOperation<()>> {
        #[cfg(test)]
        if self.force_cleanup_start_failure.is_some() {
            return Err(cleanup_error("test_database_cleanup_worker_failed"));
        }
        let exit_ticket = exit::register_cleanup_operation()?;
        let (sender, result) = tokio::sync::oneshot::channel();
        let (ready, initialized) = std::sync::mpsc::sync_channel(1);
        let (handoff, request) = std::sync::mpsc::sync_channel::<CleanupRequest>(1);
        std::thread::Builder::new()
            .name("openrust-test-db-cleanup".to_owned())
            .spawn(move || {
                let _exit = exit_ticket;
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
                    let _ = ready.send(Err(cleanup_error("test_database_cleanup_runtime_failed")));
                    return;
                };
                if ready.send(Ok(())).is_err() { return; }
                let Ok(mut request) = request.recv_timeout(std::time::Duration::from_secs(5)) else { return; };
                let outcome = runtime.block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(8),
                        cleanup_network_database(request.connection.take(), &mut request.resources),
                    ).await.map_err(|_| cleanup_error("test_database_cleanup_timeout"))?
                });
                if outcome.is_err() {
                    for resource in &mut request.resources {
                        if resource.journal.record().state != ownership::OwnershipState::Done
                            && resource.journal.record().last_error.is_none()
                            && let Err(error) = resource.journal.record_error(
                                ownership::OwnershipState::Ambiguous,
                                "test_database_cleanup_unconfirmed",
                            ) {
                                tracing::warn!(code=error.code(), "测试库失败回执未确认持久化");
                        }
                    }
                }
                runtime.shutdown_timeout(std::time::Duration::from_secs(1));
                if let Some(runtime) = request.network_runtime.take() {
                    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
                }
                if let Err(error) = &outcome {
                    tracing::warn!(database = %request.database_name, code = error.code(), "测试数据库回收未完成");
                }
                let _ = sender.send(outcome);
            })
            .map_err(|_| cleanup_error("test_database_cleanup_worker_failed"))?;
        // 执行者确认 runtime 可用后才移交连接与删除权，启动失败仍归原 guard 持有。
        initialized
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| cleanup_error("test_database_cleanup_worker_not_ready"))??;
        let request = CleanupRequest {
            connection: self.connection.take(),
            network_runtime: self.network_runtime.take(),
            resources: std::mem::take(&mut self.resources),
            database_name: self.database_name.clone(),
        };
        if let Err(error) = handoff.send(request) {
            self.connection = error.0.connection;
            self.network_runtime = error.0.network_runtime;
            self.resources = error.0.resources;
            return Err(cleanup_error("test_database_cleanup_handoff_failed"));
        }
        self.cleaned = true;
        Ok(NetworkOperation {
            result: Some(result),
        })
    }

    async fn close_sqlite_inner(mut self) -> Result<()> {
        let shutdown = if let Some(runtime) = self.sqlite_runtime.take() {
            runtime
                .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                .await
        } else {
            Ok(())
        };
        let closed = if let Some(connection) = self.connection.take() {
            connection.close().await.map_err(Error::internal)
        } else {
            Ok(())
        };
        shutdown?;
        closed?;
        cleanup_sqlite_directory(self.sqlite_directory.as_deref())?;
        self.cleaned = true;
        Ok(())
    }
}

impl fmt::Debug for TestDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestDatabase")
            .field("database_url", &"[REDACTED]")
            .field("database_name", &self.database_name)
            .field("connected", &self.connection.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        if self.cleaned || self.preserve_sqlite {
            return;
        }
        if self.sqlite {
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                return;
            };
            let connection = self.connection.take();
            let provider = self.sqlite_runtime.take();
            let directory = self.sqlite_directory.take();
            runtime.spawn(async move {
                if let Some(provider) = provider
                    && provider
                        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                        .await
                        .is_err()
                {
                    return;
                }
                if let Some(connection) = connection
                    && connection.close().await.is_err()
                {
                    return;
                }
                let _ = cleanup_sqlite_directory(directory.as_deref());
            });
            return;
        }
        match self.start_network_cleanup() {
            Ok(cleanup) => drop(cleanup),
            Err(error) => {
                tracing::warn!(database = %self.database_name, code = error.code(), "测试数据库回收线程未启动");
                // 拒收不授权删除；异步上下文不能隐式 Drop Runtime，否则会二次 panic。
                drop(self.connection.take());
                if let Some(runtime) = self.network_runtime.take() {
                    runtime.shutdown_background();
                }
            }
        }
    }
}

struct CleanupRequest {
    connection: Option<DatabaseConnection>,
    network_runtime: Option<tokio::runtime::Runtime>,
    resources: Vec<OwnedDatabase>,
    database_name: String,
}

struct NetworkOperation<T> {
    result: Option<tokio::sync::oneshot::Receiver<Result<T>>>,
}

impl<T> NetworkOperation<T> {
    async fn receive(mut self) -> Result<T> {
        self.result
            .as_mut()
            .ok_or_else(|| cleanup_error("test_database_worker_lost"))?
            .await
            .map_err(|_| cleanup_error("test_database_worker_lost"))?
    }
}

impl<T> Drop for NetworkOperation<T> {
    fn drop(&mut self) {
        // 即使当前无 entered Handle，idle current_thread runtime 也可能有待还池任务。
        // 不在任何析构路径同步等它；关闭接收端移交取消，正常退出由操作票据统一等待。
        drop(self.result.take());
    }
}

fn cleanup_error(code: &'static str) -> Error {
    Error::custom(code, "测试数据库清理失败；请核查精确自有库名并重试")
}

async fn cleanup_network_database(
    connection: Option<DatabaseConnection>,
    resources: &mut [OwnedDatabase],
) -> Result<()> {
    let pool_closed = connection.is_some();
    if let Some(connection) = connection {
        tokio::time::timeout(std::time::Duration::from_secs(2), connection.close())
            .await
            .map_err(|_| cleanup_error("test_database_pool_close_timeout"))?
            .map_err(Error::internal)?;
    }
    let mut failure = None;
    for resource in resources.iter_mut().rev() {
        let cleanup = if pool_closed {
            resource.cleanup_after_pool_close().await
        } else {
            resource.cleanup().await
        };
        if let Err(error) = cleanup
            && failure.is_none()
        {
            failure = Some(error);
        }
        if let Err(error) = resource.close_lease().await
            && failure.is_none()
        {
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

fn owned_file_source_error() -> Error {
    Error::custom(
        "test_database_owned_constructor_required",
        "SQLite 文件测试必须使用 TestDatabase::sqlite_file，不能接管已有数据库",
    )
}

fn require_backend_feature(backend: DatabaseBackend) -> Result<()> {
    if backend.is_enabled() {
        Ok(())
    } else {
        Err(Error::custom(
            "database_feature_disabled",
            format!("enable the testing {} feature", backend.feature()),
        ))
    }
}

fn sqlite_memory_url(url: &str) -> bool {
    url.strip_prefix("sqlite:")
        .map(|path| {
            path.trim_start_matches("//")
                .split('?')
                .next()
                .unwrap_or_default()
        })
        .is_some_and(|path| path == ":memory:" || path == "file::memory:")
        || url.split_once('?').is_some_and(|(_, query)| {
            url::form_urlencoded::parse(query.as_bytes())
                .any(|(key, value)| key == "mode" && value == "memory")
        })
}

fn cleanup_sqlite_directory(directory: Option<&std::path::Path>) -> Result<()> {
    let Some(directory) = directory else {
        return Ok(());
    };
    #[cfg(feature = "db-sqlite")]
    {
        crate::database::cleanup_owned_sqlite_directory(directory)
    }
    #[cfg(not(feature = "db-sqlite"))]
    {
        let _ = directory;
        Err(Error::custom(
            "database_feature_disabled",
            "清理 SQLite 测试库需要 db-sqlite",
        ))
    }
}

#[derive(Default)]
struct CreatedDatabases {
    resources: Vec<OwnedDatabase>,
}

async fn cleanup_created_databases(created: &mut CreatedDatabases) {
    for resource in created.resources.iter_mut().rev() {
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(8), resource.cleanup()).await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(database=%resource.name(),code=error.code(),"初始化失败后的测试库保留待恢复");
            }
            Err(_) => {
                if let Err(error) = resource.journal.record_error(
                    ownership::OwnershipState::Ambiguous,
                    "test_database_cleanup_timeout",
                ) {
                    tracing::warn!(code = error.code(), "测试库超时回执未确认持久化");
                }
            }
        }
        if let Err(error) = resource.close_lease().await {
            tracing::warn!(code = error.code(), "测试管理租约关闭未确认");
        }
    }
}

#[cfg(test)]
async fn connect(url: &str) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_owned());
    options.max_connections(2).min_connections(0);
    #[cfg(feature = "db-mysql")]
    if DatabaseBackend::from_url(url)? == DatabaseBackend::MySql {
        options.map_sqlx_mysql_opts(|mysql| mysql.ssl_mode(sqlx::mysql::MySqlSslMode::Required));
        options.map_sqlx_mysql_pool_opts(|pool| {
            pool.after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET time_zone = '+00:00'")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
        });
    }
    Database::connect(options).await.map_err(Error::internal)
}

fn url_for_database(base: &Url, name: &str) -> String {
    let mut url = base.clone();
    url.set_path(name);
    url.to_string()
}

fn unique_database_name() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_micros());
    let sequence = DATABASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("openrust_{}_{}_{}", std::process::id(), timestamp, sequence)
}

#[cfg(test)]
mod tests {
    use super::{TestDatabase, connect, unique_database_name, url_for_database};
    use crate::{Error, Result, database::DatabaseTableNames};
    use sea_orm::{ConnectionTrait, DbErr, Statement};
    use sea_orm_migration::{MigrationName, MigrationTrait, MigratorTrait, SchemaManager};
    use url::Url;

    #[tokio::test(flavor = "current_thread")]
    async fn failed_cleanup_start_never_drops_owned_runtime_in_async_context() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(Error::internal)?;
        let database = TestDatabase {
            database_name: "openrust_unowned_no_sql".to_owned(),
            database_url: "mysql://unreachable/never_used".to_owned(),
            connection: None,
            cleaned: false,
            sqlite_runtime: None,
            sqlite_directory: None,
            sqlite: false,
            preserve_sqlite: false,
            network_runtime: Some(runtime),
            resources: Vec::new(),
            force_cleanup_start_failure: Some(()),
        };
        let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(database)));
        assert!(dropped.is_ok(), "回收执行者拒收不能让异步析构 panic");
        Ok(())
    }

    struct EmptyMigrator;

    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }

    struct FailingMigrator;
    struct ControlledFailure;

    impl MigrationName for ControlledFailure {
        fn name(&self) -> &'static str {
            "controlled_test_database_failure"
        }
    }

    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for ControlledFailure {
        async fn up(&self, _manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            Err(DbErr::Custom("controlled migration failure".into()))
        }

        async fn down(&self, _manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }
    }

    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for FailingMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(ControlledFailure)]
        }
    }

    async fn sentinel(admin: &str, name: String) -> Result<TestDatabase> {
        let mut database = TestDatabase::create_named_with_table_names::<EmptyMigrator>(
            admin,
            DatabaseTableNames::default(),
            name,
        )
        .await?;
        let connection = database
            .connection
            .take()
            .ok_or_else(|| Error::custom("test_database_pool_missing", "哨兵数据库缺少连接池"))?;
        let seeded = async {
            connection
                .execute_unprepared("CREATE TABLE ownership_sentinel (id INTEGER PRIMARY KEY)")
                .await?;
            connection
                .execute_unprepared("INSERT INTO ownership_sentinel (id) VALUES (73)")
                .await?;
            Ok::<_, DbErr>(())
        }
        .await;
        // 不让已有连接阻止 PostgreSQL DROP，避免旧清理漏洞被连接占用掩盖。
        let closed = connection.close().await;
        seeded.map_err(Error::internal)?;
        closed.map_err(Error::internal)?;
        Ok(database)
    }

    async fn verify_sentinel(database: &mut TestDatabase) -> Result<()> {
        let connection = connect(database.url()).await?;
        let row = connection
            .query_one_raw(Statement::from_string(
                connection.get_database_backend(),
                "SELECT id FROM ownership_sentinel".to_owned(),
            ))
            .await;
        // 即使断言失败，也交回 guard，由原 close 路径关闭连接并回收本测试哨兵库。
        database.connection = Some(connection);
        let row = row
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("test_database_sentinel_missing", "已有库哨兵丢失"))?;
        assert_eq!(row.try_get::<i32>("", "id").map_err(Error::internal)?, 73);
        Ok(())
    }

    #[tokio::test]
    async fn failed_database_create_preserves_existing_database_and_sentinel() -> Result<()> {
        let Ok(admin) = crate::test_database_environment::database_url() else {
            eprintln!("skipping database ownership collision: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let name = unique_database_name();
        let mut existing = sentinel(&admin, name.clone()).await?;
        let outcome = async {
            let collision = TestDatabase::create_named_with_table_names::<EmptyMigrator>(
                &admin,
                DatabaseTableNames::default(),
                name,
            )
            .await;
            assert!(collision.is_err());
            verify_sentinel(&mut existing).await
        }
        .await;
        let cleanup = existing.close().await;
        outcome?;
        cleanup
    }

    #[cfg(feature = "db-postgres")]
    #[tokio::test]
    async fn failed_template_create_preserves_existing_template_and_target_sentinels() -> Result<()>
    {
        let Ok(admin) = crate::test_database_environment::database_url() else {
            eprintln!("skipping template ownership collision: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        if super::DatabaseBackend::from_url(&admin)? != super::DatabaseBackend::PostgreSql {
            eprintln!("skipping PostgreSQL template collision: test backend is not PostgreSQL");
            return Ok(());
        }
        let name = unique_database_name();
        let mut existing = sentinel(&admin, name.clone()).await?;
        let mut template = match sentinel(&admin, format!("{name}_template")).await {
            Ok(template) => template,
            Err(error) => {
                let _ = existing.close().await;
                return Err(error);
            }
        };
        let outcome = async {
            let collision = TestDatabase::create_named_with_table_names::<EmptyMigrator>(
                &admin,
                DatabaseTableNames::default(),
                name,
            )
            .await;
            assert!(collision.is_err());
            verify_sentinel(&mut existing).await?;
            verify_sentinel(&mut template).await
        }
        .await;
        let template_cleanup = template.close().await;
        let target_cleanup = existing.close().await;
        outcome?;
        template_cleanup?;
        target_cleanup
    }

    #[tokio::test]
    async fn failed_owned_migration_cleans_only_successfully_created_databases() -> Result<()> {
        let Ok(admin) = crate::test_database_environment::database_url() else {
            eprintln!("skipping owned migration failure: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let name = unique_database_name();
        let failed = TestDatabase::create_named_with_table_names::<FailingMigrator>(
            &admin,
            DatabaseTableNames::default(),
            name.clone(),
        )
        .await;
        assert!(failed.is_err());
        // 严格 CREATE（无 IF NOT EXISTS）能重用同名库，证明失败后回收了自有目标/模板。
        let recovered = TestDatabase::create_named_with_table_names::<EmptyMigrator>(
            &admin,
            DatabaseTableNames::default(),
            name,
        )
        .await?;
        recovered.close().await
    }

    #[cfg(all(unix, feature = "db-mysql"))]
    fn mysql_cleanup_admin() -> Result<Option<String>> {
        let Ok(admin) = crate::test_database_environment::database_url() else {
            eprintln!("skipping MySQL close barrier: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        if super::DatabaseBackend::from_url(&admin)? != super::DatabaseBackend::MySql {
            eprintln!("skipping MySQL close barrier: test backend is not MySQL");
            return Ok(None);
        }
        Ok(Some(admin))
    }

    #[cfg(all(unix, feature = "db-mysql"))]
    async fn released_cleanup_record(
        resource_id: &str,
    ) -> Result<super::ownership::OwnershipRecord> {
        let journal = super::ownership::OwnershipJournal::new(&super::resources::state_root()?)?;
        let path = super::resources::state_root()?.join(format!("{resource_id}.json"));
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some(record) = journal.open_try_locked(&path)? {
                    return record.load();
                }
                // close 的结果先通知调用者；等待清理线程确实交回本机持责锁。
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| super::cleanup_error("test_database_journal_release_timeout"))?
    }

    #[cfg(all(unix, feature = "db-mysql"))]
    #[tokio::test]
    async fn mysql_normal_cleanup_rechecks_observed_busy_until_controlled_connection_closes()
    -> Result<()> {
        let Some(admin) = mysql_cleanup_admin()? else {
            return Ok(());
        };
        let database = TestDatabase::create_named_with_table_names::<EmptyMigrator>(
            &admin,
            DatabaseTableNames::default(),
            unique_database_name(),
        )
        .await?;
        let resource_id = database.resources[0].journal.record().resource_id.clone();
        let busy = std::sync::Arc::new(tokio::sync::Notify::new());
        assert!(
            database.resources[0]
                .lease()?
                .cleanup_busy_observer
                .set(busy.clone())
                .is_ok()
        );
        let controlled = connect(database.url()).await?;
        controlled
            .execute_unprepared("SELECT 1")
            .await
            .map_err(Error::internal)?;
        let (cleanup, release) = tokio::join!(database.close(), async {
            // 必须先实际观察到 busy，不能以任意等待替代服务端活动屏障。
            let observed =
                tokio::time::timeout(std::time::Duration::from_secs(3), busy.notified()).await;
            let closed = controlled.close().await.map_err(Error::internal);
            observed.map_err(|_| super::cleanup_error("test_database_busy_not_observed"))?;
            closed
        });
        release?;
        cleanup?;
        assert_eq!(
            released_cleanup_record(&resource_id).await?.state,
            super::ownership::OwnershipState::Done
        );
        Ok(())
    }

    #[cfg(all(unix, feature = "db-mysql"))]
    #[tokio::test]
    async fn mysql_normal_cleanup_retains_persistent_foreign_connection_and_specific_journal()
    -> Result<()> {
        let Some(admin) = mysql_cleanup_admin()? else {
            return Ok(());
        };
        let database = TestDatabase::create_named_with_table_names::<EmptyMigrator>(
            &admin,
            DatabaseTableNames::default(),
            unique_database_name(),
        )
        .await?;
        let record = database.resources[0].journal.record().clone();
        let busy = std::sync::Arc::new(tokio::sync::Notify::new());
        assert!(
            database.resources[0]
                .lease()?
                .cleanup_busy_observer
                .set(busy.clone())
                .is_ok()
        );
        let foreign = connect(database.url()).await?;
        foreign
            .execute_unprepared("SELECT 1")
            .await
            .map_err(Error::internal)?;
        let closing = database.close();
        let observed =
            tokio::time::timeout(std::time::Duration::from_secs(3), busy.notified()).await;
        let cleanup = closing.await;
        let outcome = async {
            observed.map_err(|_| super::cleanup_error("test_database_busy_not_observed"))?;
            let error = cleanup.err().ok_or_else(|| {
                super::cleanup_error("test_database_foreign_connection_was_not_retained")
            })?;
            assert_eq!(error.code(), "test_database_resource_in_use");
            let retained = released_cleanup_record(&record.resource_id).await?;
            assert_eq!(retained.state, super::ownership::OwnershipState::Ambiguous);
            assert_eq!(
                retained.last_error.as_deref(),
                Some("test_database_resource_in_use")
            );
            assert_eq!(retained.marker_identity, record.marker_identity);
            // 原严格恢复同样不能删除仍被外部使用的数据库。
            TestDatabase::recover(&admin).await?;
            let retained = released_cleanup_record(&record.resource_id).await?;
            assert_eq!(retained.state, super::ownership::OwnershipState::Ambiguous);
            assert_eq!(
                retained.last_error.as_deref(),
                Some("test_database_resource_in_use")
            );
            let marker = foreign
                .query_one_raw(Statement::from_string(
                    sea_orm::DbBackend::MySql,
                    "SELECT CAST(COUNT(*) AS SIGNED) AS n FROM information_schema.routines WHERE ROUTINE_SCHEMA=DATABASE() AND ROUTINE_NAME='__openrust_test_database_owner' AND ROUTINE_TYPE='PROCEDURE'".to_owned(),
                ))
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| super::cleanup_error("test_database_marker_missing"))?;
            assert_eq!(marker.try_get::<i64>("", "n").map_err(Error::internal)?, 1);
            Ok::<(), Error>(())
        }
        .await;
        let closed = foreign.close().await.map_err(Error::internal);
        outcome?;
        closed?;
        // 只观察已正常关闭连接的服务端退出，不赋予恢复路径重试删除权限。
        let lease = super::lease::ServerLease::acquire(&admin, &record.database_name).await?;
        let idle = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                match lease.require_idle(&record.database_name).await {
                    Ok(()) => return Ok(()),
                    Err(error) if error.code() == "test_database_resource_in_use" => {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => return Err(error),
                }
            }
        })
        .await
        .map_err(|_| super::cleanup_error("test_database_activity_timeout"))?;
        let released = lease.close().await;
        idle?;
        released?;
        TestDatabase::recover(&admin).await?;
        assert_eq!(
            released_cleanup_record(&record.resource_id).await?.state,
            super::ownership::OwnershipState::Done
        );
        Ok(())
    }

    #[test]
    fn database_url_replacement_preserves_auth_and_query_options() {
        let base = Url::parse("postgres://tester:secret@localhost/app_db?sslmode=require");
        assert!(base.is_ok());
        let Ok(base) = base else {
            return;
        };
        let isolated = Url::parse(&url_for_database(&base, "openrust_test_1"));
        assert!(isolated.is_ok());
        let Ok(isolated) = isolated else {
            return;
        };
        assert_eq!(isolated.username(), "tester");
        assert_eq!(isolated.password(), Some("secret"));
        assert_eq!(isolated.path(), "/openrust_test_1");
        assert_eq!(isolated.query(), Some("sslmode=require"));
    }

    #[test]
    fn generated_database_names_fit_postgresql_identifier_limits() {
        let name = unique_database_name();
        assert!(name.len() <= 54);
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        );
    }
}

#[cfg(test)]
#[path = "database/sqlite_tests.rs"]
mod sqlite_tests;

#[cfg(all(test, feature = "db-sqlite"))]
mod sqlite_managed_migration_tests {
    use super::*;
    use sea_orm::{DbBackend, Statement};
    use sea_orm_migration::prelude::*;

    struct RebuildMigrator;
    struct NullableMigration;
    impl MigrationName for NullableMigration {
        fn name(&self) -> &'static str {
            "m20261105_000001_fixture_nullable_owner"
        }
    }
    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for NullableMigration {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            let names = DatabaseTableNames::current()
                .ok_or_else(|| DbErr::Migration("fixture缺少标准表名作用域".into()))?;
            let physical = names
                .resolve("fixture_accounts")
                .map_err(|error| DbErr::Migration(error.to_string()))?;
            let quoted = names
                .quote("fixture_accounts", DbBackend::Sqlite)
                .map_err(|error| DbErr::Migration(error.to_string()))?;
            manager
                .get_connection()
                .execute_unprepared(&format!(
                    "CREATE TABLE {quoted} (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE)"
                ))
                .await?;
            manager
                .get_connection()
                .execute_unprepared(&format!(
                    "INSERT INTO {quoted} VALUES (1,'retained@example.test')"
                ))
                .await?;
            // 该API明确要求标准SQLite迁移执行者；旧fixture直接M::up会在此失败。
            crate::database::dialect::set_column_nullable(
                manager,
                Alias::new(physical),
                ColumnDef::new(Alias::new("email")).text().take(),
                true,
            )
            .await
        }
        async fn down(&self, _manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }
    }
    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for RebuildMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(NullableMigration)]
        }
    }

    async fn verify_nullable(database: &TestDatabase, physical: &str) -> Result<()> {
        let connection = database.connection()?;
        connection
            .execute_unprepared(&format!("INSERT INTO \"{physical}\" VALUES (2,NULL)"))
            .await
            .map_err(Error::internal)?;
        let row = connection
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT COUNT(*) AS n FROM \"{physical}\""),
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("fixture_missing_row", "缺少fixture数据"))?;
        assert_eq!(row.try_get::<i64>("", "n").map_err(Error::internal)?, 2);
        let app = database
            .sqlite_runtime
            .as_ref()
            .ok_or_else(|| Error::custom("fixture_app_missing", "缺少标准App"))?;
        let db = app.state().resolve::<crate::database::Db>().await?;
        assert_eq!(db.table_names().resolve("fixture_accounts")?, physical);
        assert_eq!(
            app.section::<crate::config::DatabaseConfig>("database")?
                .max_connections,
            2
        );
        // 标准migrate再次执行应使用同一历史，不重复CREATE或丢弃现有记录。
        app.run_command("migrate", Vec::new()).await?;
        Ok(())
    }
    #[tokio::test]
    async fn sqlite_memory_and_owned_file_run_nullable_rebuild_through_standard_app() -> Result<()>
    {
        for file in [false, true] {
            let database =
                TestDatabase::create_sqlite::<RebuildMigrator>(file, DatabaseTableNames::default())
                    .await?;
            let result = verify_nullable(&database, "fixture_accounts").await;
            let cleanup = database.close().await;
            result?;
            cleanup?;
        }
        Ok(())
    }
    #[tokio::test]
    async fn sqlite_standard_fixture_preserves_arbitrary_mapping_and_prefix() -> Result<()> {
        for (names, physical) in [
            (
                DatabaseTableNames::new("tenant_", std::collections::BTreeMap::new())?,
                "tenant_fixture_accounts",
            ),
            (
                DatabaseTableNames::new(
                    "tenant_",
                    std::collections::BTreeMap::from([(
                        "fixture_accounts".into(),
                        "custom_accounts".into(),
                    )]),
                )?,
                "custom_accounts",
            ),
        ] {
            let database = TestDatabase::create_sqlite::<RebuildMigrator>(false, names).await?;
            let result = verify_nullable(&database, physical).await;
            let cleanup = database.close().await;
            result?;
            cleanup?;
        }
        Ok(())
    }
}
