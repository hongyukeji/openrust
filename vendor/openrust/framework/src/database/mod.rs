#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::config::ConfigError;
use crate::core::{Error, RateLimiter, Result};
use crate::support::BoxFuture;
use sea_orm::ConnectionTrait;
use sea_orm::{ConnectOptions, Database, Statement, TransactionTrait};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;

tokio::task_local! {
    static PRIMARY_CONNECTION_SCOPE: (usize, DatabaseConnection);
}

mod batch;
mod coordination;
pub mod dialect;
/// Eloquent通用模型元数据、持久身份与属性转换基础。
pub mod eloquent;
pub(crate) mod migration;
mod migration_lock;
pub mod query;
mod rate_limit;
mod seed;
#[cfg(any(test, feature = "db-sqlite"))]
mod sqlite;

#[cfg(feature = "db-sqlite")]
pub(crate) fn cleanup_owned_sqlite_directory(directory: &std::path::Path) -> Result<()> {
    sqlite::migration_lock::cleanup_owned_directory(directory)
}
#[cfg(all(test, feature = "db-sqlite", feature = "auth-fortify"))]
#[path = "../../tests/unit/database/final_checks.rs"]
mod final_check_tests;
pub mod schema;
mod table_names;

pub use crate::config::database::{DatabaseConfig, DatabaseInstanceConfig};
pub use batch::{
    BatchCursor, BatchDirection, BatchOptions, BatchReader, BatchReport, MAX_BATCH_SIZE,
};
pub use coordination::{
    CoordinationMigration, IdempotencyClaim, IdempotencyLease, IdempotencyResponse, LockAcquire,
    LockLease, RequestIdempotency, SharedLocks,
};
pub use migration::{FrameworkMigrator, MigrationServiceProvider};
pub use query::{
    JoinKind, PageContext, QueryBuilder, QueryPage, RawSql, SimplePage, SortDirection, WhereGroup,
    with_page_context,
};
pub use rate_limit::{DatabaseRateLimitMigration, DatabaseRateLimitStore};
pub use sea_orm;
pub use sea_orm::{DatabaseConnection, DatabaseTransaction, DbBackend};
pub use sea_orm_migration;
pub use seed::SeedServiceProvider;
pub use table_names::DatabaseTableNames;

/// 由内置驱动 feature 控制连接器的数据库后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    /// PostgreSQL 14 或更高版本；日常开发与 CI 使用 PostgreSQL 18。
    PostgreSql,
    /// MySQL 8.0 或更高版本；日常开发与 CI 使用 MySQL 8.4 LTS。
    MySql,
    /// SQLite 3.26.0 或更高版本；单机文件或内存数据库。
    Sqlite,
}

impl DatabaseBackend {
    /// 根据连接 URL scheme 识别一种官方支持的后端。
    ///
    /// # Errors
    ///
    /// URL scheme 不属于官方支持的后端时，返回配置错误。
    pub fn from_url(url: &str) -> Result<Self> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Ok(Self::PostgreSql)
        } else if url.starts_with("mysql://") {
            Ok(Self::MySql)
        } else if url.starts_with("sqlite:") {
            Ok(Self::Sqlite)
        } else {
            Err(Error::custom(
                "unsupported_database_backend",
                "OpenRust 支持 PostgreSQL、MySQL 和 SQLite 连接地址",
            ))
        }
    }

    /// 返回 `openrust` package 是否启用了此后端对应的 Cargo feature。
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::PostgreSql => cfg!(feature = "db-postgres"),
            Self::MySql => cfg!(feature = "db-mysql"),
            Self::Sqlite => cfg!(feature = "db-sqlite"),
        }
    }

    /// 编译此后端所需的 Cargo feature。
    pub const fn feature(self) -> &'static str {
        match self {
            Self::PostgreSql => "db-postgres",
            Self::MySql => "db-mysql",
            Self::Sqlite => "db-sqlite",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::PostgreSql => "PostgreSQL",
            Self::MySql => "MySQL",
            Self::Sqlite => "SQLite",
        }
    }
}

/// 对 [`DatabaseServiceProvider`] 注册的数据库连接池的可克隆句柄。
#[derive(Clone, Default)]
pub struct Db {
    connections: Arc<OnceLock<DatabaseConnections>>,
    migration_lock_url: Arc<OnceLock<String>>,
    table_names: Arc<DatabaseTableNames>,
    next_replica: Arc<AtomicUsize>,
    events: Arc<OnceLock<crate::events::Dispatcher>>,
}

/// 由实际发起事务的 [`Db`] 签发、仅在本次事务回调内有效的借用视图。
///
/// 来源句柄与原始事务由引擎绑定；这不是认证或同意证明。消费方仍须通过
/// [`Self::transaction`] 执行原子操作，并验证实际表、租户与主体。
/// 不同句柄即使用相同 URL，也不能冒充本视图的来源。
///
/// 字段与构造器不公开，不能包装任意原始事务：
/// ```compile_fail
/// use openrust::database::{DatabaseTransaction, TransactionContext};
/// fn replace<'a>(mut context: TransactionContext<'a>, other: &'a DatabaseTransaction) {
///     context.transaction = other;
/// }
/// ```
pub struct TransactionContext<'tx> {
    origin: Arc<OnceLock<DatabaseConnections>>,
    transaction: &'tx DatabaseTransaction,
}

impl<'tx> TransactionContext<'tx> {
    /// 返回引擎实际持有的同一个事务；不赋予提交或回滚所有权。
    pub fn transaction(&self) -> &'tx DatabaseTransaction {
        self.transaction
    }

    /// 核验指定句柄或其克隆是否为本次事务来源。
    ///
    /// 同一句柄并发开启的两个事务都会返回 true，本方法不区分事务。
    /// 查询必须使用各自的 [`Self::transaction`]；认证、表与租户仍须另外核验。
    pub fn is_from(&self, db: &Db) -> bool {
        Arc::ptr_eq(&self.origin, &db.connections)
    }
}

/// 应用中已配置数据库连接的只读选择器。
///
/// `Databases` 只选择 OpenRust 的 `Db` 句柄，不提供任意服务查找或全局访问。
#[derive(Clone)]
pub struct Databases {
    default: Db,
    handles: Arc<BTreeMap<String, Db>>,
    guardable_columns: Arc<RwLock<BTreeMap<String, Arc<[String]>>>>,
}

impl Databases {
    fn new(default: Db, handles: BTreeMap<String, Db>) -> Self {
        Self {
            default,
            handles: Arc::new(handles),
            guardable_columns: Arc::default(),
        }
    }

    /// 返回与容器解析 `Db` 相同的默认数据库句柄。
    pub fn default_database(&self) -> Db {
        self.default.clone()
    }

    /// 按名称返回数据库句柄。
    ///
    /// # Errors
    ///
    /// 未配置此名称时返回配置错误。
    pub fn get(&self, name: &str) -> Result<Db> {
        self.handles.get(name).cloned().ok_or_else(|| {
            Error::custom(
                "database_instance_not_found",
                "requested database instance is not configured",
            )
        })
    }

    // 同一应用按类缓存首次成功的非空清单；Schema I/O 不持缓存锁。
    async fn guardable_columns(&self, model: &eloquent::ModelMetadata) -> Result<Arc<[String]>> {
        if let Some(columns) = self.guardable_columns.read().await.get(&model.class_name) {
            return Ok(Arc::clone(columns));
        }
        let db = model.resolve_connection(self, None)?;
        let columns: Arc<[String]> = db.schema().get_column_listing(&model.table).await?.into();
        if columns.is_empty() {
            return Ok(columns);
        }
        let mut cache = self.guardable_columns.write().await;
        Ok(Arc::clone(
            cache.entry(model.class_name.clone()).or_insert(columns),
        ))
    }

    /// 返回 default 与所有具名连接的名称。
    pub fn names(&self) -> Vec<String> {
        let mut names = self.handles.keys().cloned().collect::<Vec<_>>();
        names.sort_by_key(|name| name != "default");
        names
    }
}

/// 派发给事务监听器、且仍可回滚的提交前边界。
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionBeforeCommit;

impl crate::events::Event for TransactionBeforeCommit {
    const NAME: &'static str = r"OpenRust\Database\Events\TransactionBeforeCommit";
}

/// 仅在事务成功提交后派发的进程内通知。
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionCommitted;

impl crate::events::Event for TransactionCommitted {
    const NAME: &'static str = r"OpenRust\Database\Events\TransactionCommitted";
}

type FinalCheckFn = fn(&DatabaseTransaction) -> BoxFuture<'_, Result<()>>;
const NO_FINAL_CHECK: Option<FinalCheckFn> = None;

#[derive(Clone, Copy)]
enum ReadRoute {
    Primary,
    Replica(usize),
}

#[derive(Clone)]
struct DatabaseConnections {
    primary: DatabaseConnection,
    read_replicas: Vec<DatabaseConnection>,
}

impl Db {
    #[cfg(feature = "auth")]
    pub(crate) fn same_handle(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.connections, &other.connections)
    }

    /// 安装自定义数据库 Provider 提供的 SeaORM 连接。
    ///
    /// 自定义 Provider 在 register 阶段登记异步工厂；工厂建立连接后先登记受管关闭，再调用此方法并返回句柄。
    /// 当前通用初始化支持 PostgreSQL、MySQL 和 SQLite SeaORM 连接；其他后端会因缺少
    /// [`DatabaseTableNames`] quoting 策略而返回错误。这样连接、事务、迁移和种子操作可复用同一个框架服务；
    /// 自定义 Provider 仍须验证应用所需的能力，且不能据此推断框架的方言专属能力可用。
    ///
    /// # Errors
    ///
    /// 此句柄已初始化时返回错误。
    pub fn initialize(&self, connection: DatabaseConnection) -> Result<()> {
        self.initialize_with_replicas(connection, Vec::new())
    }

    /// 为自定义数据库 Provider 安装主库连接池和可选的只读副本连接池。
    ///
    /// 所有连接池必须使用相同后端。框架不会配置或验证数据库复制；调用方必须确保副本为只读，并接受复制延迟。
    ///
    /// # Errors
    ///
    /// 任一连接池使用了不同后端，或此句柄已初始化时返回错误。
    pub fn initialize_with_replicas(
        &self,
        primary: DatabaseConnection,
        read_replicas: Vec<DatabaseConnection>,
    ) -> Result<()> {
        let backend = primary.get_database_backend();
        self.table_names.validate_backend(backend)?;
        if read_replicas
            .iter()
            .any(|replica| replica.get_database_backend() != backend)
        {
            return Err(Error::custom(
                "database_replica_backend_mismatch",
                "all database read replicas must use the same backend as the primary",
            ));
        }
        self.connections
            .set(DatabaseConnections {
                primary,
                read_replicas,
            })
            .map_err(|_| {
                Error::custom(
                    "database_already_initialized",
                    "database connection pools were initialized more than once",
                )
            })
    }

    /// 使用传入的不可变表名解析器创建数据库句柄。
    pub fn with_table_names(table_names: DatabaseTableNames) -> Self {
        Self {
            connections: Arc::new(OnceLock::new()),
            migration_lock_url: Arc::new(OnceLock::new()),
            table_names: Arc::new(table_names),
            next_replica: Arc::new(AtomicUsize::new(0)),
            events: Arc::new(OnceLock::new()),
        }
    }

    /// 为迁移命令指定同一物理主库的独立互斥连接地址；自定义 Provider 在初始化后调用。
    ///
    /// 凭据只保留在句柄中，不进入 `Debug` 或公开错误。地址必须指向已安装的主库。
    ///
    /// # Errors
    ///
    /// 地址格式、后端不匹配或重复配置时失败；实际物理数据库身份在迁移取锁时再次核验。
    pub fn configure_migration_lock_url(&self, url: impl Into<String>) -> Result<()> {
        let url = url.into();
        validate_database_url(&url, "migration lock URL")?;
        let backend = match DatabaseBackend::from_url(&url)? {
            DatabaseBackend::PostgreSql => DbBackend::Postgres,
            DatabaseBackend::MySql => DbBackend::MySql,
            DatabaseBackend::Sqlite => DbBackend::Sqlite,
        };
        if backend != self.backend()? {
            return Err(Error::custom(
                "migration_lock_backend_mismatch",
                "迁移锁连接与主库后端不同",
            ));
        }
        self.migration_lock_url
            .set(url)
            .map_err(|_| Error::custom("migration_lock_already_configured", "迁移锁连接已配置"))
    }

    pub(crate) fn migration_lock_url(&self) -> Result<&str> {
        self.migration_lock_url
            .get()
            .map(String::as_str)
            .ok_or_else(|| {
                Error::custom(
                    "migration_lock_connection_unavailable",
                    "迁移连接缺少独立锁连接地址",
                )
            })
    }

    fn install_events(&self, events: crate::events::Dispatcher) {
        let _ = self.events.set(events);
    }

    #[cfg(feature = "auth")]
    pub(crate) fn events(&self) -> Option<crate::events::Dispatcher> {
        self.events.get().cloned()
    }

    /// 返回逻辑表名对应的已配置物理表名。
    pub fn table_name(&self, logical_name: &str) -> Result<String> {
        let backend = self.backend()?;
        self.table_names.resolve(logical_name).and_then(|name| {
            self.table_names.quote(logical_name, backend)?;
            Ok(name)
        })
    }

    /// 返回经过后端引用符处理的已配置物理表名。
    pub fn quoted_table_name(&self, logical_name: &str) -> Result<String> {
        self.table_names.quote(logical_name, self.backend()?)
    }

    /// 克隆此数据库句柄使用的不可变解析器。
    pub fn table_names(&self) -> DatabaseTableNames {
        self.table_names.as_ref().clone()
    }

    /// 返回此连接已初始化的 SeaORM 后端。
    ///
    /// # Errors
    ///
    /// 连接池尚未初始化时返回内部错误。
    pub fn backend(&self) -> Result<DbBackend> {
        Ok(self.primary_connection()?.get_database_backend())
    }

    pub(crate) fn query_date_format(&self) -> Result<&'static str> {
        match self.backend()? {
            DbBackend::MySql | DbBackend::Postgres | DbBackend::Sqlite => Ok("Y-m-d H:i:s"),
            _ => Err(query::unsupported_backend()),
        }
    }

    /// 返回已初始化 SeaORM 连接池的克隆。
    ///
    /// # Errors
    ///
    /// 数据库服务尚未完成初始化时返回内部错误。
    pub fn connection(&self) -> Result<DatabaseConnection> {
        self.primary_connection()
    }

    /// 返回主库 SeaORM 连接池的克隆。
    pub fn primary_connection(&self) -> Result<DatabaseConnection> {
        self.connections
            .get()
            .map(|connections| connections.primary.clone())
            .ok_or_else(|| Error::internal(DatabaseNotReady))
    }

    /// 以轮询方式返回一个读连接；没有副本时返回主库连接。
    ///
    /// 调用方只能在此连接上执行可接受复制延迟的只读操作。此方法不会检查 SQL，也不会在查询出错后重试或回退。
    /// 在 [`with_primary`](Self::with_primary) 作用域中，此方法始终返回主库连接。
    pub fn read_connection(&self) -> Result<DatabaseConnection> {
        self.read_route_connection(self.read_route()?)
    }

    fn read_route(&self) -> Result<ReadRoute> {
        let connections = self
            .connections
            .get()
            .ok_or_else(|| Error::internal(DatabaseNotReady))?;
        let handle_id = Arc::as_ptr(&self.connections) as usize;
        if PRIMARY_CONNECTION_SCOPE
            .try_with(|(scope_id, _)| *scope_id == handle_id)
            .unwrap_or(false)
            || connections.read_replicas.is_empty()
        {
            return Ok(ReadRoute::Primary);
        }
        Ok(ReadRoute::Replica(next_replica_index(
            connections.read_replicas.len(),
            &self.next_replica,
        )))
    }

    fn read_route_connection(&self, route: ReadRoute) -> Result<DatabaseConnection> {
        let connections = self
            .connections
            .get()
            .ok_or_else(|| Error::internal(DatabaseNotReady))?;
        Ok(match route {
            ReadRoute::Primary => connections.primary.clone(),
            ReadRoute::Replica(index) => connections.read_replicas[index].clone(),
        })
    }

    async fn check_readiness(&self) -> Result<()> {
        let connections = self
            .connections
            .get()
            .ok_or_else(|| Error::internal(DatabaseNotReady))?;
        connections.primary.ping().await.map_err(Error::internal)?;
        for replica in &connections.read_replicas {
            replica.ping().await.map_err(Error::internal)?;
        }
        Ok(())
    }

    /// 在任务本地作用域内运行 future；作用域中的 [`read_connection`](Self::read_connection) 会返回此句柄的主库连接。
    ///
    /// 此作用域不会传播到独立创建的 Tokio task。为同一句柄嵌套作用域时，内层 future 完成后会恢复外层作用域。
    pub async fn with_primary<F: std::future::Future>(&self, future: F) -> Result<F::Output> {
        let primary = self.primary_connection()?;
        Ok(PRIMARY_CONNECTION_SCOPE
            .scope((Arc::as_ptr(&self.connections) as usize, primary), future)
            .await)
    }

    /// 在数据库事务中运行操作。
    ///
    /// 回调成功后，在标准 [`DatabaseServiceProvider`] 与 `EventsServiceProvider` 同时安装时会先派发
    /// [`TransactionBeforeCommit`]；该事件任一监听器失败都会回滚事务。提交成功后尽力派发
    /// [`TransactionCommitted`] 的 direct listeners；其错误只记录稳定错误码，不改变已提交结果。
    /// 可靠的提交后副作用应在提交前事件中排入同一事务的队列。
    ///
    /// # Errors
    ///
    /// 连接池、开始、提交或回滚失败时返回内部错误；回滚成功后保留回调或提交前监听器的原始错误。
    pub async fn transaction<T, F>(&self, callback: F) -> Result<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
    {
        self.transaction_impl(callback, false, NO_FINAL_CHECK).await
    }

    /// 为依赖读取后写入的认证安全操作取得同一数据库事务所有权。
    /// SQLite在首次读取前取得写锁；其他后端保留行锁SQL和原事务行为。
    #[cfg(feature = "auth")]
    pub(crate) async fn transaction_for_update<T, F>(&self, callback: F) -> Result<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
    {
        self.transaction_impl(callback, true, NO_FINAL_CHECK).await
    }

    /// 在同一主库事务内执行操作，并在直接提交前监听器返回后做最后校验。
    ///
    /// 操作、直接监听器或末检失败时回滚；末检成功后直接开始提交。
    /// 排队监听器只等待任务入队，不等待其 worker 执行；不能将末检当作队列执行后的校验。
    /// 调用方须用条件写入与锁定读取维护当前权威，本方法不自动锁定业务记录，
    /// 也不提供跨连接原子性。提交返回错误时，远端是否已经提交仍可能未知。
    /// 提交后监听器失败不能撤销已经提交的数据。
    pub async fn transaction_for_update_checked<T, F, C>(
        &self,
        callback: F,
        final_check: C,
    ) -> Result<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
        C: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<()>> + Send,
    {
        self.transaction_impl(callback, true, Some(final_check))
            .await
    }

    /// 向工作回调及最后校验签发有来源的同事务借用视图。
    ///
    /// 复用唯一事务引擎：工作、提交前监听器及队列入队均完成后，执行最后校验，
    /// 随后直接开始提交；不等待队列 worker。SQLite先取得写事务，其他后端仍需适当行锁。
    /// 本入口不依赖认证或 Fortify，也不提供跨库及外部服务原子性。
    ///
    /// 最后校验可异步读取，期限应在其最后一次 await 后同步检查。
    /// 提交开始后的取消或错误可能无法确认耐久结果，不应自动重试签发。
    ///
    /// # Errors
    ///
    /// 开始、提交或回滚失败返回内部错误；回滚成功时保留工作、监听器或最后校验的原错误。
    pub async fn transaction_for_update_checked_context<T, F, C>(
        &self,
        callback: F,
        final_check: C,
    ) -> Result<T>
    where
        T: Send,
        F: for<'tx> FnOnce(TransactionContext<'tx>) -> BoxFuture<'tx, Result<T>> + Send,
        C: for<'tx> FnOnce(TransactionContext<'tx>) -> BoxFuture<'tx, Result<()>> + Send,
    {
        let work_origin = Arc::clone(&self.connections);
        let check_origin = Arc::clone(&self.connections);
        self.transaction_for_update_checked(
            move |transaction| {
                callback(TransactionContext {
                    origin: work_origin,
                    transaction,
                })
            },
            move |transaction| {
                final_check(TransactionContext {
                    origin: check_origin,
                    transaction,
                })
            },
        )
        .await
    }

    async fn transaction_impl<T, F, C>(
        &self,
        callback: F,
        for_update: bool,
        final_check: Option<C>,
    ) -> Result<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
        C: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<()>> + Send,
    {
        // 事务回调可能捕获大对象，避免把引擎的挂起状态嵌入上层 future。
        Box::pin(self.transaction_with_state_impl(
            &mut (),
            |(), transaction| callback(transaction),
            for_update,
            final_check,
        ))
        .await
    }

    // 调用者的可变状态与事务借用共享同一次回调生命周期。
    pub(crate) async fn transaction_with_state<S, T, F>(
        &self,
        state: &mut S,
        callback: F,
    ) -> Result<T>
    where
        S: Send,
        T: Send,
        F: for<'a> FnOnce(&'a mut S, &'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
    {
        Box::pin(self.transaction_with_state_impl(state, callback, false, NO_FINAL_CHECK)).await
    }

    async fn transaction_with_state_impl<S, T, F, C>(
        &self,
        state: &mut S,
        callback: F,
        for_update: bool,
        final_check: Option<C>,
    ) -> Result<T>
    where
        S: Send,
        T: Send,
        F: for<'a> FnOnce(&'a mut S, &'a DatabaseTransaction) -> BoxFuture<'a, Result<T>> + Send,
        C: for<'a> FnOnce(&'a DatabaseTransaction) -> BoxFuture<'a, Result<()>> + Send,
    {
        let events = self.events.get().cloned();
        let connection = self.connection()?;
        let transaction = if for_update {
            begin_for_update(&connection).await?
        } else {
            connection.begin().await.map_err(Error::internal)?
        };
        let value = match callback(state, &transaction).await {
            Ok(value) => value,
            Err(error) => {
                transaction.rollback().await.map_err(Error::internal)?;
                return Err(error);
            }
        };
        if let Some(events) = &events
            && let Err(error) = events
                .dispatch_in_transaction(&transaction, &TransactionBeforeCommit)
                .await
        {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(error);
        }
        if let Some(check) = final_check
            && let Err(error) = check(&transaction).await
        {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(error);
        }
        transaction.commit().await.map_err(Error::internal)?;
        if let Some(events) = self.events.get()
            && let Err(error) = events.dispatch_direct(&TransactionCommitted).await
        {
            tracing::error!(
                error_code = error.code(),
                "事务已提交，但提交后监听器执行失败"
            );
        }
        Ok(value)
    }
}

/// SQLite用同一连接的写事务替代安全操作所需的行级排他，不改变普通事务默认。
pub(crate) async fn begin_for_update(
    connection: &DatabaseConnection,
) -> Result<DatabaseTransaction> {
    if connection.get_database_backend() == DbBackend::Sqlite {
        connection
            .begin_with_options(sea_orm::TransactionOptions {
                sqlite_transaction_mode: Some(sea_orm::SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await
            .map_err(Error::internal)
    } else {
        connection.begin().await.map_err(Error::internal)
    }
}

fn next_replica_index(replica_count: usize, counter: &AtomicUsize) -> usize {
    counter.fetch_add(1, Ordering::Relaxed) % replica_count
}

impl fmt::Debug for Db {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Db")
            .field("initialized", &self.connections.get().is_some())
            .field("table_names", &self.table_names)
            .finish_non_exhaustive()
    }
}

/// 声明数据库服务，并在启动阶段建立配置的连接池。
#[derive(Debug, Default, Clone, Copy)]
pub struct DatabaseServiceProvider;

fn database_instance_schema() -> crate::config::repository::SectionSchema {
    use crate::config::repository::SectionSchema as S;
    S::object([
        ("url", S::String.nullable().sensitive()),
        ("read_replicas", S::array(S::String.sensitive())),
        ("foreign_key_constraints", S::Boolean.nullable()),
        ("busy_timeout", S::any().nullable().sensitive()),
        ("journal_mode", S::any().nullable().sensitive()),
        ("synchronous", S::any().nullable().sensitive()),
        ("pragmas", S::map(S::any().sensitive())),
        ("max_connections", S::Integer),
        ("min_connections", S::Integer),
        ("connect_timeout_secs", S::Integer),
        ("acquire_timeout_secs", S::Integer),
        ("table_prefix", S::String),
        ("table_names", S::map(S::String)),
    ])
}

fn database_schema() -> crate::config::repository::SectionSchema {
    use crate::config::repository::SectionSchema as S;
    if let S::Object { mut fields, .. } = database_instance_schema() {
        fields.insert("instances".into(), S::map(database_instance_schema()));
        fields.insert("migrations".into(), S::object([("table", S::String)]));
        S::object(fields)
    } else {
        S::any()
    }
}

fn database_handles(config: &DatabaseConfig) -> Result<Databases> {
    validate_config(config)?;
    let db = Db::with_table_names(DatabaseTableNames::new(
        config.table_prefix.clone(),
        config.table_names.clone(),
    )?);
    let mut handles = BTreeMap::from([("default".to_owned(), db.clone())]);
    for (name, instance) in &config.instances {
        let names =
            DatabaseTableNames::new(instance.table_prefix.clone(), instance.table_names.clone())?;
        handles.insert(name.clone(), Db::with_table_names(names));
    }
    Ok(Databases::new(db, handles))
}

impl crate::support::ServiceProvider for DatabaseServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.config_section(
                "database",
                DatabaseConfig::default(),
                database_schema(),
                validate_config,
            )?;
            app.singleton::<DatabaseConfig, _>(|resolver| {
                Box::pin(
                    async move { Ok(Arc::new(resolver.section::<DatabaseConfig>("database")?)) },
                )
            })
            .await?;
            let repository = app.config_repository();
            app.singleton::<Databases, _>(move |resolver| {
                let repository = repository.clone();
                Box::pin(async move {
                    let application_root = repository
                        .get("application_root")?
                        .filter(|value| !value.is_null())
                        .map(serde_json::from_value::<std::path::PathBuf>)
                        .transpose()
                        .map_err(|_| ConfigError::diagnostic("application_root", "invalid_path"))?;
                    let config = resolver.section::<DatabaseConfig>("database")?;
                    let databases = database_handles(&config)?;
                    let resources = resolver.resources()?;
                    for name in databases.names() {
                        let handle = databases.get(&name)?;
                        let instance = if name == "default" {
                            DatabaseInstanceConfig::from(&config)
                        } else {
                            config.instances.get(&name).cloned().ok_or_else(|| {
                                Error::custom(
                                    "database_config_missing",
                                    "registered instance configuration missing",
                                )
                            })?
                        };
                        if name == "default" && instance.url.is_none() {
                            continue;
                        }
                        connect_instance_managed(
                            &handle,
                            &instance,
                            &name,
                            Some(&resources),
                            application_root.as_deref(),
                        )
                        .await?;
                    }
                    Ok(Arc::new(databases))
                })
            })
            .await?;
            app.singleton::<Db, _>(|resolver| {
                Box::pin(async move {
                    Ok(Arc::new(
                        resolver.resolve::<Databases>().await?.default_database(),
                    ))
                })
            })
            .await?;
            app.singleton::<RateLimiter, _>(|resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    Ok(Arc::new(RateLimiter::new(DatabaseRateLimitStore::new(
                        (*db).clone(),
                    ))))
                })
            })
            .await?;
            app.require::<Databases>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.section::<DatabaseConfig>("database")?;
            validate_config(&config)?;
            let databases = app.resolve::<Databases>().await?;
            let events = if app.is_bound::<crate::events::Dispatcher>() {
                Some(app.resolve::<crate::events::Dispatcher>().await?)
            } else {
                None
            };
            let readiness = if app.is_bound::<crate::core::Readiness>() {
                Some(app.resolve::<crate::core::Readiness>().await?)
            } else {
                None
            };
            for name in databases.names() {
                let handle = databases.get(&name)?;
                if let Some(events) = &events {
                    handle.install_events((**events).clone());
                }
                if name == "default" && config.url.is_none() {
                    continue;
                }
                if let Some(readiness) = &readiness {
                    let database = handle.clone();
                    readiness.register_probe(format!("database.{name}"), move || {
                        let database = database.clone();
                        async move { database.check_readiness().await }
                    })?;
                }
            }
            Ok(())
        })
    }
}

fn validate_config(config: &DatabaseConfig) -> Result<()> {
    validate_instance("default", &DatabaseInstanceConfig::from(config), false)?;
    for (name, instance) in &config.instances {
        if !valid_instance_name(name) || name == "default" {
            return Err(Error::Config(ConfigError::Invalid(format!(
                "database.instances.{name} has an invalid or reserved name"
            ))));
        }
        validate_instance(&format!("instances.{name}"), instance, true)?;
    }
    Ok(())
}

fn valid_instance_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

fn validate_instance(key: &str, config: &DatabaseInstanceConfig, require_url: bool) -> Result<()> {
    DatabaseTableNames::new(config.table_prefix.clone(), config.table_names.clone())?;
    if config.max_connections == 0
        || config.min_connections > config.max_connections
        || config.connect_timeout_secs == 0
        || config.acquire_timeout_secs == 0
    {
        return Err(Error::Config(ConfigError::Invalid(format!(
            "database.{key} pool sizes and timeouts must be positive, and min_connections must not exceed max_connections"
        ))));
    }
    if config.url.is_none() && !config.read_replicas.is_empty() {
        return Err(Error::Config(ConfigError::Invalid(format!(
            "database.{key}.read_replicas requires a primary URL"
        ))));
    }
    if require_url && config.url.is_none() {
        return Err(Error::Config(ConfigError::Invalid(format!(
            "missing database.{key}.url"
        ))));
    }
    if let Some(url) = config.url.as_deref() {
        let backend = DatabaseBackend::from_url(url).map_err(|_| {
            Error::Config(ConfigError::Invalid(format!(
                "database.{key}.url has an unsupported database URL"
            )))
        })?;
        validate_database_url(url, &format!("database.{key}.url"))?;
        if !backend.is_enabled() {
            return Err(Error::Config(ConfigError::Invalid(format!(
                "database.{key}.url requires the openrust feature `{}`",
                backend.feature()
            ))));
        }
        for (index, replica_url) in config.read_replicas.iter().enumerate() {
            let replica_backend = DatabaseBackend::from_url(replica_url).map_err(|_| {
                Error::Config(ConfigError::Invalid(format!(
                    "database.{key}.read_replicas[{index}] has an unsupported database URL"
                )))
            })?;
            validate_database_url(
                replica_url,
                &format!("database.{key}.read_replicas[{index}]"),
            )?;
            if !replica_backend.is_enabled() {
                return Err(Error::Config(ConfigError::Invalid(format!(
                    "database.{key}.read_replicas[{index}] requires the openrust feature `{}`",
                    replica_backend.feature()
                ))));
            }
            if replica_backend != backend {
                return Err(Error::Config(ConfigError::Invalid(format!(
                    "database.{key}.read_replicas[{index}] must use the same backend as database.{key}.url"
                ))));
            }
        }
    }
    Ok(())
}

fn validate_database_url(url: &str, key: &str) -> Result<()> {
    if let Some(database) = url.strip_prefix("sqlite:") {
        if database.is_empty() || database.contains('\0') {
            return Err(ConfigError::Invalid(format!("{key} has an invalid SQLite URL")).into());
        }
        #[cfg(feature = "db-sqlite")]
        {
            use std::str::FromStr as _;
            if !database.trim_start_matches("//").starts_with("file:") {
                sqlx::sqlite::SqliteConnectOptions::from_str(url).map_err(|_| {
                    ConfigError::Invalid(format!("{key} has an invalid SQLite URL"))
                })?;
            }
        }
        return Ok(());
    }
    url::Url::parse(url).map_err(|_| {
        Error::Config(ConfigError::Invalid(format!(
            "{key} has an invalid database URL"
        )))
    })?;
    Ok(())
}

async fn connect_instance_managed(
    db: &Db,
    config: &DatabaseInstanceConfig,
    name: &str,
    resources: Option<&crate::foundation::resources::ResourceRegistrar>,
    application_root: Option<&std::path::Path>,
) -> Result<()> {
    let url = config.url.as_deref().ok_or_else(|| {
        let message = if name == "default" {
            "missing database.url (set DB_CONNECTION and DB_* fields, or configure [database].url / APP_DATABASE_URL)".to_owned()
        } else {
            format!("missing database.instances.{name}.url")
        };
        Error::Config(ConfigError::Invalid(message))
    })?;
    let backend = DatabaseBackend::from_url(url)?;
    let primary_role = if name == "default" {
        "primary".to_owned()
    } else {
        format!("primary for database.instances.{name}")
    };
    let connection = connect_database(
        url,
        config,
        backend,
        &primary_role,
        resources,
        application_root,
    )
    .await?;
    let mut replicas = Vec::with_capacity(config.read_replicas.len());
    for (index, replica_url) in config.read_replicas.iter().enumerate() {
        let replica_backend = DatabaseBackend::from_url(replica_url)?;
        replicas.push(
            connect_database(
                replica_url,
                config,
                replica_backend,
                &format!("read replica {index} for database.{name}"),
                resources,
                application_root,
            )
            .await?,
        );
    }
    db.initialize_with_replicas(connection, replicas)?;
    db.configure_migration_lock_url(url)
}

async fn connect_database(
    url: &str,
    config: &DatabaseInstanceConfig,
    backend: DatabaseBackend,
    role: &str,
    resources: Option<&crate::foundation::resources::ResourceRegistrar>,
    application_root: Option<&std::path::Path>,
) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_owned());
    options
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs));
    #[cfg(feature = "db-mysql")]
    if backend == DatabaseBackend::MySql {
        options.map_sqlx_mysql_opts(|mysql_options| {
            mysql_options.ssl_mode(sqlx::mysql::MySqlSslMode::Required)
        });
    }
    #[cfg(not(feature = "db-sqlite"))]
    let _ = application_root;
    #[cfg(feature = "db-sqlite")]
    if backend == DatabaseBackend::Sqlite {
        use sqlx::ConnectOptions as _;
        let prepared = sqlite::connection_settings(url, config, application_root)?;
        let native_foreign_keys = sqlite::native::foreign_keys_default().await?;
        let initial_pragmas = sqlite::initial_pragma_value(&prepared.pragmas, native_foreign_keys);
        let sqlite_options = prepared
            .options
            .pragma("foreign_keys", initial_pragmas)
            .disable_statement_logging();
        options = ConnectOptions::new(prepared.url);
        options
            .max_connections(if prepared.single_connection {
                1
            } else {
                config.max_connections
            })
            .min_connections(if prepared.single_connection {
                config.min_connections.min(1)
            } else {
                config.min_connections
            })
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs));
        options.map_sqlx_sqlite_opts(move |_| sqlite_options.clone());
        let in_memory = prepared.in_memory;
        // SQLx after_connect 会重连并记录原错误；初始化批次必须在 connect 内执行。
        options.map_sqlx_sqlite_pool_opts(move |pool| {
            if in_memory {
                pool.idle_timeout(None).max_lifetime(None)
            } else {
                pool
            }
        });
    }
    #[cfg(feature = "db-mysql")]
    if backend == DatabaseBackend::MySql {
        // SeaORM after_connect 是池级回调；会话时区必须随每条物理连接设置。
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
    let connection = Database::connect(options).await.map_err(|_| {
        Error::Config(ConfigError::Invalid(format!(
            "could not connect to configured {} {role}",
            backend.name()
        )))
    })?;
    if let Some(resources) = resources {
        let pool = connection.clone();
        if let Err(error) = resources.on_shutdown(move || {
            Box::pin(async move { pool.close_by_ref().await.map_err(Error::internal) })
        }) {
            let _ = connection.close_by_ref().await;
            return Err(error);
        }
    }
    validate_server_version(&connection, backend).await?;
    Ok(connection)
}

async fn validate_server_version(
    connection: &sea_orm::DatabaseConnection,
    backend: DatabaseBackend,
) -> Result<()> {
    let (sea_backend, query) = match backend {
        DatabaseBackend::PostgreSql => (
            DbBackend::Postgres,
            "SELECT current_setting('server_version_num') AS version",
        ),
        DatabaseBackend::MySql => (DbBackend::MySql, "SELECT VERSION() AS version"),
        DatabaseBackend::Sqlite => (DbBackend::Sqlite, "SELECT sqlite_version() AS version"),
    };
    let version = connection
        .query_one_raw(Statement::from_string(sea_backend, query.to_owned()))
        .await
        .map_err(|_| {
            Error::Config(ConfigError::Invalid(format!(
                "could not verify configured {} database version",
                backend.name()
            )))
        })?
        .and_then(|row| row.try_get::<String>("", "version").ok())
        .ok_or_else(|| {
            Error::Config(ConfigError::Invalid(format!(
                "could not verify configured {} database version",
                backend.name()
            )))
        })?;
    if !is_server_version_supported(&version, backend) {
        let minimum = match backend {
            DatabaseBackend::PostgreSql => "14",
            DatabaseBackend::MySql => "8.0",
            DatabaseBackend::Sqlite => "3.26.0",
        };
        return Err(Error::Config(ConfigError::Invalid(format!(
            "configured {} database version is unsupported; OpenRust requires {minimum} or newer",
            backend.name()
        ))));
    }
    Ok(())
}

fn is_server_version_supported(version: &str, backend: DatabaseBackend) -> bool {
    match backend {
        DatabaseBackend::PostgreSql => version
            .parse::<u32>()
            .is_ok_and(|server_version| server_version >= 140_000),
        DatabaseBackend::Sqlite => {
            let parts = version
                .split('.')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>();
            matches!(parts, Ok(parts) if parts.len() == 3 && (parts[0], parts[1], parts[2]) >= (3, 26, 0))
        }
        DatabaseBackend::MySql => {
            if version.to_ascii_lowercase().contains("mariadb") {
                return false;
            }
            let mut parts = version.split('.');
            let major = parts.next().and_then(|part| part.parse::<u32>().ok());
            let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
            matches!((major, minor), (Some(major), Some(minor)) if (major, minor) >= (8, 0))
        }
    }
}

#[derive(Debug)]
struct DatabaseNotReady;

impl std::fmt::Display for DatabaseNotReady {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("database plugin has not initialized the connection pool")
    }
}

impl std::error::Error for DatabaseNotReady {}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use figment::{Figment, providers::Serialized};
    use serde_json::json;

    struct CaptureFirstPool {
        url: String,
        captured: Arc<std::sync::Mutex<Option<DatabaseConnection>>>,
    }

    impl crate::support::ServiceProvider for CaptureFirstPool {
        fn boot<'a>(
            &'a self,
            app: &'a mut crate::foundation::BootContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let pool = sea_orm::Database::connect(self.url.as_str())
                    .await
                    .map_err(Error::internal)?;
                pool.ping().await.map_err(Error::internal)?;
                let closing = pool.clone();
                app.resources().on_shutdown(move || {
                    Box::pin(async move { closing.close_by_ref().await.map_err(Error::internal) })
                })?;
                *self
                    .captured
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(pool);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn failed_provider_second_connection_closes_first_pool() -> Result<()> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!(
                "skipping real provider startup cleanup: OPENRUST_TEST_DATABASE_URL is unset"
            );
            return Ok(());
        };
        let backend = DatabaseBackend::from_url(&url)?;
        if !backend.is_enabled() {
            eprintln!("skipping real provider startup cleanup: backend feature is disabled");
            return Ok(());
        }
        let mut missing = url::Url::parse(&url).map_err(Error::internal)?;
        let name = format!(
            "openrust_r2_missing_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(Error::internal)?
                .as_nanos()
        );
        missing.set_path(&format!("/{name}"));
        let captured = Arc::new(std::sync::Mutex::new(None));
        let mut providers = crate::foundation::ProviderBuilder::new();
        providers
            .provider(CaptureFirstPool {
                url: url.clone(),
                captured: Arc::clone(&captured),
            })
            .provider(DatabaseServiceProvider);
        let result = providers
            .start(Figment::from(Serialized::defaults(json!({
                "database": {
                    "url": url,
                    "min_connections": 1,
                    "instances": {
                        "second": {"url": missing.as_str(), "min_connections": 1}
                    }
                }
            }))))
            .await;
        assert!(
            result.is_err(),
            "second pool must fail during provider boot"
        );
        let pool = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| Error::custom("test", "first database pool was not captured"))?;
        assert!(
            pool.ping().await.is_err(),
            "startup must close the first pool"
        );
        Ok(())
    }

    struct PendingAfterFirstPool(Arc<tokio::sync::Notify>);

    impl crate::support::ServiceProvider for PendingAfterFirstPool {
        fn boot<'a>(
            &'a self,
            _: &'a mut crate::foundation::BootContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0.notify_one();
                std::future::pending::<()>().await;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn cancelled_provider_start_closes_first_real_pool() -> Result<()> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping cancelled real provider pool: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let captured = Arc::new(std::sync::Mutex::new(None));
        let entered = Arc::new(tokio::sync::Notify::new());
        let mut providers = crate::foundation::ProviderBuilder::new();
        providers
            .provider(CaptureFirstPool {
                url,
                captured: Arc::clone(&captured),
            })
            .provider(PendingAfterFirstPool(Arc::clone(&entered)));
        let startup = tokio::spawn(async move { providers.start(Figment::new()).await });
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .map_err(Error::internal)?;
        let pool = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| Error::custom("test", "first database pool was not captured"))?;
        pool.ping().await.map_err(Error::internal)?;
        startup.abort();
        let _ = startup.await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if pool.ping().await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(Error::internal)?;
        Ok(())
    }

    #[tokio::test]
    async fn real_second_pool_failure_closes_first_pool() -> Result<()> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping real provider pool cleanup: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let backend = DatabaseBackend::from_url(&url)?;
        let config = DatabaseInstanceConfig {
            min_connections: 1,
            connect_timeout_secs: 2,
            ..DatabaseInstanceConfig::default()
        };
        let owner = crate::foundation::resources::ResourceOwner::new();
        let registrar = owner.registrar();
        let first = connect_database(
            &url,
            &config,
            backend,
            "provider cleanup first pool",
            Some(&registrar),
            None,
        )
        .await?;
        first.ping().await.map_err(Error::internal)?;
        let mut missing = url::Url::parse(&url).map_err(Error::internal)?;
        let name = format!(
            "openrust_r2_missing_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(Error::internal)?
                .as_nanos()
        );
        missing.set_path(&format!("/{name}"));
        let second = connect_database(
            missing.as_str(),
            &config,
            backend,
            "provider cleanup second pool",
            Some(&registrar),
            None,
        )
        .await;
        assert!(second.is_err(), "missing database must fail during connect");
        owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        assert!(first.ping().await.is_err(), "first pool must be closed");
        Ok(())
    }

    struct Overlay;
    impl crate::support::ServiceProvider for Overlay {
        fn boot<'a>(
            &'a self,
            app: &'a mut crate::foundation::BootContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("database", json!({"table_prefix": "new_"})) })
        }
    }

    #[tokio::test]
    async fn missing_database_is_optional_and_overlay_configures_shared_handles() -> Result<()> {
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(Overlay).provider(DatabaseServiceProvider);
        let runtime = builder.start(Figment::new()).await?;
        let databases = runtime.container().resolve::<Databases>().await?;
        let db = runtime.container().resolve::<Db>().await?;
        assert_eq!(db.table_names().resolve("posts")?, "new_posts");
        assert_eq!(
            databases
                .default_database()
                .table_names()
                .resolve("posts")?,
            "new_posts"
        );
        assert!(db.connection().is_err());
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct LateReadiness(Arc<AtomicUsize>);
    impl crate::support::ServiceProvider for LateReadiness {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let calls = Arc::clone(&self.0);
                app.singleton::<crate::core::Readiness, _>(move |_| {
                    let calls = Arc::clone(&calls);
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Arc::new(crate::core::Readiness::default()))
                    })
                })
                .await?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn boot_resolves_readiness_registered_by_later_provider() -> Result<()> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder
            .provider(DatabaseServiceProvider)
            .provider(LateReadiness(Arc::clone(&calls)));
        let runtime = builder.start(Figment::new()).await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct BrokenReadiness;
    impl crate::support::ServiceProvider for BrokenReadiness {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.singleton::<crate::core::Readiness, _>(|resolver| {
                    Box::pin(async move {
                        resolver.resolve::<u128>().await?;
                        Ok(Arc::new(crate::core::Readiness::default()))
                    })
                })
                .await?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn bound_readiness_factory_failure_is_not_treated_as_optional() {
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder
            .provider(DatabaseServiceProvider)
            .provider(BrokenReadiness);
        let error = builder.start(Figment::new()).await.err();
        assert!(error.is_some(), "bound readiness factory must fail startup");
    }

    struct MalformedLateDatabaseUrl;
    impl crate::support::ServiceProvider for MalformedLateDatabaseUrl {
        fn boot<'a>(
            &'a self,
            app: &'a mut crate::foundation::BootContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay("database", json!({"url": "postgres://[secret-sentinel"}))
            })
        }
    }

    #[tokio::test]
    async fn malformed_late_database_url_fails_final_validation_without_leaking_secret() {
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder
            .provider(DatabaseServiceProvider)
            .provider(MalformedLateDatabaseUrl);
        let error = builder.start(Figment::new()).await.err();
        assert!(error.is_some());
        if let Some(error) = error {
            assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
        }
    }

    #[tokio::test]
    async fn invalid_database_and_replica_without_primary_fail_before_connecting() {
        for database in [
            json!({"max_connections": 0}),
            json!({"read_replicas": ["postgres://private-sentinel"]}),
            json!({"url": "bad-secret-sentinel"}),
        ] {
            let mut builder = crate::foundation::ProviderBuilder::new();
            builder.provider(DatabaseServiceProvider);
            let result = builder
                .start(Figment::from(Serialized::defaults(
                    json!({"database":database}),
                )))
                .await;
            assert!(result.is_err());
            if let Err(error) = result {
                assert!(!format!("{error:?} {error}").contains("sentinel"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "db-mysql")]
    #[tokio::test]
    async fn mysql_every_physical_connection_uses_utc()
    -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("skipping physical MySQL UTC test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        if super::DatabaseBackend::from_url(&url)? != super::DatabaseBackend::MySql {
            return Ok(());
        }
        let config = super::DatabaseInstanceConfig {
            max_connections: 2,
            min_connections: 0,
            ..Default::default()
        };
        let database = super::connect_database(
            &url,
            &config,
            super::DatabaseBackend::MySql,
            "UTC test",
            None,
            None,
        )
        .await?;
        let pool = database.get_mysql_connection_pool();
        let mut first = pool.acquire().await?;
        let mut second = pool.acquire().await?;
        for physical in [&mut *first, &mut *second] {
            let zone: String = sqlx::query_scalar("SELECT @@session.time_zone")
                .fetch_one(physical)
                .await?;
            assert_eq!(zone, "+00:00");
        }
        drop(first);
        drop(second);
        database.close().await?;
        Ok(())
    }
    use super::*;
    use figment::providers::Format;

    #[test]
    fn defaults_define_a_small_pool_without_a_connection_url() {
        let config = DatabaseConfig::default();
        assert_eq!(config.url, None);
        assert_eq!(config.read_replicas, Vec::<String>::new());
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 1);
        assert_eq!(config.connect_timeout_secs, 5);
        assert_eq!(config.acquire_timeout_secs, 30);
    }

    #[test]
    fn debug_redacts_connection_url() {
        let config = DatabaseConfig {
            url: Some("postgres://user:secret@localhost/db".into()),
            ..DatabaseConfig::default()
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("secret"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn debug_does_not_render_replica_urls() {
        let config = DatabaseConfig {
            read_replicas: vec!["postgres://replica:secret@host/db".into()],
            ..DatabaseConfig::default()
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("replica@"));
        assert!(rendered.contains("1 configured"));
    }

    #[test]
    fn named_instance_config_debug_redacts_primary_and_replica_urls() {
        let mut instances = BTreeMap::new();
        instances.insert(
            "analytics".to_owned(),
            DatabaseInstanceConfig {
                url: Some("postgres://named:password@localhost/analytics".into()),
                read_replicas: vec!["postgres://replica:secret@localhost/analytics".into()],
                ..DatabaseInstanceConfig::default()
            },
        );
        let config = DatabaseConfig {
            instances,
            ..DatabaseConfig::default()
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("password"));
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("named@"));
        assert!(!rendered.contains("replica@"));
    }

    #[test]
    fn named_instances_reject_invalid_names_and_missing_urls() {
        for name in ["", "Default", "bad.name", "has space"] {
            assert!(!valid_instance_name(name), "unexpectedly accepted {name:?}");
        }
        assert!(!valid_instance_name(&"a".repeat(64)));
        let mut config = DatabaseConfig::default();
        config
            .instances
            .insert("analytics".to_owned(), DatabaseInstanceConfig::default());
        assert!(validate_config(&config).is_err());
        config.instances.clear();
        config.instances.insert(
            "default".to_owned(),
            DatabaseInstanceConfig {
                url: Some("postgres://localhost/db".into()),
                ..DatabaseInstanceConfig::default()
            },
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn database_config_deserializes_complete_named_instances() {
        let figment = figment::Figment::new().merge(figment::providers::Toml::string(
            "[database]\nurl = 'postgres://default/db'\n[database.instances.analytics]\nurl = 'postgres://analytics/db'\nmax_connections = 4\ntable_prefix = 'analytics_'\n",
        ));
        let config = figment.extract_inner::<DatabaseConfig>("database");
        assert!(config.is_ok());
        let Some(config) = config.ok() else { return };
        let named = config.instances.get("analytics");
        assert!(named.is_some_and(|named| {
            named.max_connections == 4
                && named.table_prefix == "analytics_"
                && named.url.as_deref() == Some("postgres://analytics/db")
        }));
    }

    #[test]
    fn database_selector_keeps_default_handle_and_rejects_unknown_names() {
        let default = Db::default();
        let databases = Databases::new(
            default.clone(),
            BTreeMap::from([("default".to_owned(), default.clone())]),
        );
        assert!(Arc::ptr_eq(
            &default.connections,
            &databases.default_database().connections
        ));
        assert!(
            databases
                .get("default")
                .is_ok_and(|selected| Arc::ptr_eq(&default.connections, &selected.connections))
        );
        assert_eq!(
            databases
                .get("missing")
                .err()
                .map(|error| error.code().to_owned()),
            Some("database_instance_not_found".to_owned())
        );
        let error = databases.get("postgres://user:secret@host/database").err();
        assert!(error.is_some_and(|error| !error.to_string().contains("secret")));
    }

    #[test]
    fn replica_selection_round_robins_without_skipping_configured_entries() {
        let counter = AtomicUsize::new(0);
        let selected = (0..5)
            .map(|_| next_replica_index(3, &counter))
            .collect::<Vec<_>>();
        assert_eq!(selected, [0, 1, 2, 0, 1]);
    }

    #[test]
    fn connection_is_unavailable_before_plugin_initialization() {
        assert!(Db::default().connection().is_err());
    }

    #[test]
    fn invalid_pool_settings_are_rejected() {
        let config = DatabaseConfig {
            min_connections: 11,
            ..DatabaseConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn database_url_requires_its_backend_feature() {
        let config = DatabaseConfig {
            url: Some("postgres://user:secret@localhost/db".into()),
            ..DatabaseConfig::default()
        };
        assert_eq!(
            validate_config(&config).is_ok(),
            cfg!(feature = "db-postgres")
        );
    }

    #[test]
    fn replica_backend_requires_its_feature_and_matches_primary() {
        let postgres = DatabaseConfig {
            url: Some("postgres://primary@localhost/db".into()),
            read_replicas: vec!["mysql://replica@localhost/db".into()],
            ..DatabaseConfig::default()
        };
        assert!(validate_config(&postgres).is_err());

        let missing_feature = DatabaseConfig {
            url: Some("postgres://primary@localhost/db".into()),
            read_replicas: vec!["postgres://replica@localhost/db".into()],
            ..DatabaseConfig::default()
        };
        assert_eq!(
            validate_config(&missing_feature).is_ok(),
            cfg!(feature = "db-postgres")
        );
    }

    #[test]
    fn official_backend_detection_rejects_unknown_url_schemes() {
        assert!(matches!(
            DatabaseBackend::from_url("postgresql://user@host/db"),
            Ok(DatabaseBackend::PostgreSql)
        ));
        assert!(matches!(
            DatabaseBackend::from_url("mysql://user@host/db"),
            Ok(DatabaseBackend::MySql)
        ));
        assert!(matches!(
            DatabaseBackend::from_url("sqlite::memory:"),
            Ok(DatabaseBackend::Sqlite)
        ));
    }

    #[test]
    fn provisional_minimum_database_versions_are_checked_explicitly() {
        assert!(is_server_version_supported(
            "3.26.0",
            DatabaseBackend::Sqlite
        ));
        assert!(is_server_version_supported(
            "3.51.1",
            DatabaseBackend::Sqlite
        ));
        assert!(!is_server_version_supported(
            "3.25.3",
            DatabaseBackend::Sqlite
        ));
        assert!(!is_server_version_supported(
            "3.26",
            DatabaseBackend::Sqlite
        ));
        assert!(!is_server_version_supported(
            "unknown",
            DatabaseBackend::Sqlite
        ));
        assert!(is_server_version_supported(
            "140000",
            DatabaseBackend::PostgreSql
        ));
        assert!(is_server_version_supported(
            "160000",
            DatabaseBackend::PostgreSql
        ));
        assert!(!is_server_version_supported(
            "130022",
            DatabaseBackend::PostgreSql
        ));
        assert!(is_server_version_supported(
            "8.0.44",
            DatabaseBackend::MySql
        ));
        assert!(is_server_version_supported(
            "8.4.13",
            DatabaseBackend::MySql
        ));
        assert!(!is_server_version_supported(
            "5.7.44",
            DatabaseBackend::MySql
        ));
        assert!(!is_server_version_supported(
            "10.6.18-MariaDB",
            DatabaseBackend::MySql
        ));
        assert!(!is_server_version_supported(
            "unknown",
            DatabaseBackend::MySql
        ));
    }
}

#[cfg(all(test, feature = "db-mysql"))]
#[path = "transaction_context_mysql_tests.rs"]
mod transaction_context_mysql_tests;
