#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::config::ConfigError;
use crate::core::{BoxFuture, Error, RateLimiter, Result};
use sea_orm::ConnectionTrait;
use sea_orm::{ConnectOptions, Database, Statement, TransactionTrait};
use std::collections::BTreeMap;
use std::sync::OnceLock;

tokio::task_local! {
    static PRIMARY_CONNECTION_SCOPE: (usize, DatabaseConnection);
}

mod batch;
mod coordination;
pub mod dialect;
pub(crate) mod migration;
mod migration_lock;
mod rate_limit;
mod seed;
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
pub use rate_limit::{DatabaseRateLimitMigration, DatabaseRateLimitStore};
pub use sea_orm;
pub use sea_orm::{DatabaseConnection, DatabaseTransaction, DbBackend};
pub use sea_orm_migration;
pub use seed::SeedServiceProvider;
pub use table_names::DatabaseTableNames;

/// OpenRust 官方 PostgreSQL/MySQL feature 集支持的数据库后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    /// PostgreSQL 14 或更高版本；日常开发与 CI 使用 PostgreSQL 18。
    PostgreSql,
    /// MySQL 8.0 或更高版本；日常开发与 CI 使用 MySQL 8.4 LTS。
    MySql,
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
        } else {
            Err(Error::custom(
                "unsupported_database_backend",
                "OpenRust officially supports PostgreSQL and MySQL database URLs",
            ))
        }
    }

    /// 返回 `openrust` package 是否启用了此后端对应的 Cargo feature。
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::PostgreSql => cfg!(feature = "db-postgres"),
            Self::MySql => cfg!(feature = "db-mysql"),
        }
    }

    /// 编译此后端所需的 Cargo feature。
    pub const fn feature(self) -> &'static str {
        match self {
            Self::PostgreSql => "db-postgres",
            Self::MySql => "db-mysql",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::PostgreSql => "PostgreSQL",
            Self::MySql => "MySQL",
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
    events: Arc<OnceLock<crate::events::Events>>,
}

/// 应用中已配置数据库连接的只读选择器。
///
/// `Databases` 只选择 OpenRust 的 `Db` 句柄，不提供任意服务查找或全局访问。
#[derive(Clone)]
pub struct Databases {
    default: Db,
    handles: Arc<BTreeMap<String, Db>>,
}

impl Databases {
    fn new(default: Db, handles: BTreeMap<String, Db>) -> Self {
        Self {
            default,
            handles: Arc::new(handles),
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

/// 仅在事务成功提交后派发的进程内通知。
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionCommitted;

#[derive(Clone)]
struct DatabaseConnections {
    primary: DatabaseConnection,
    read_replicas: Vec<DatabaseConnection>,
}

impl Db {
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

    fn install_events(&self, events: crate::events::Events) {
        let _ = self.events.set(events);
    }

    #[cfg(feature = "auth")]
    pub(crate) fn events(&self) -> Option<crate::events::Events> {
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
        let connections = self
            .connections
            .get()
            .ok_or_else(|| Error::internal(DatabaseNotReady))?;
        let handle_id = Arc::as_ptr(&self.connections) as usize;
        if let Ok(Some(connection)) = PRIMARY_CONNECTION_SCOPE
            .try_with(|(scope_id, connection)| (*scope_id == handle_id).then(|| connection.clone()))
        {
            return Ok(connection);
        }
        if connections.read_replicas.is_empty() {
            return Ok(connections.primary.clone());
        }
        let index = next_replica_index(connections.read_replicas.len(), &self.next_replica);
        Ok(connections.read_replicas[index].clone())
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
        let events = self.events.get().cloned();
        let transaction = self.connection()?.begin().await.map_err(Error::internal)?;
        let value = match callback(&transaction).await {
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

impl crate::core::ServiceProvider for DatabaseServiceProvider {
    fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
        app.config_section(
            "database",
            DatabaseConfig::default(),
            database_schema(),
            validate_config,
        )?;
        app.singleton::<DatabaseConfig, _>(|resolver| {
            Box::pin(async move { Ok(Arc::new(resolver.section::<DatabaseConfig>("database")?)) })
        });
        app.singleton::<Databases, _>(|resolver| {
            Box::pin(async move {
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
                    connect_instance_managed(&handle, &instance, &name, Some(&resources)).await?;
                }
                Ok(Arc::new(databases))
            })
        });
        app.singleton::<Db, _>(|resolver| {
            Box::pin(async move {
                Ok(Arc::new(
                    resolver.resolve::<Databases>().await?.default_database(),
                ))
            })
        });
        app.singleton::<RateLimiter, _>(|resolver| {
            Box::pin(async move {
                let db = resolver.resolve::<Db>().await?;
                Ok(Arc::new(RateLimiter::new(DatabaseRateLimitStore::new(
                    (*db).clone(),
                ))))
            })
        });
        app.require::<Databases>();
        Ok(())
    }

    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.section::<DatabaseConfig>("database")?;
            validate_config(&config)?;
            let databases = app.resolve::<Databases>().await?;
            let events = if app.is_bound::<crate::events::Events>() {
                Some(app.resolve::<crate::events::Events>().await?)
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
    resources: Option<&crate::core::resources::ResourceRegistrar>,
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
    let connection = connect_database(url, config, backend, &primary_role, resources).await?;
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
    resources: Option<&crate::core::resources::ResourceRegistrar>,
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
    #[cfg(feature = "db-mysql")]
    if backend == DatabaseBackend::MySql {
        options.after_connect(|connection| {
            Box::pin(async move {
                connection
                    .execute_unprepared("SET time_zone = '+00:00'")
                    .await?;
                Ok(())
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

    impl crate::core::ServiceProvider for CaptureFirstPool {
        fn boot<'a>(
            &'a self,
            app: &'a mut crate::core::BootContext<'_>,
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
        let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
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
        let mut providers = crate::core::ProviderBuilder::new();
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

    impl crate::core::ServiceProvider for PendingAfterFirstPool {
        fn boot<'a>(
            &'a self,
            _: &'a mut crate::core::BootContext<'_>,
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
        let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping cancelled real provider pool: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let captured = Arc::new(std::sync::Mutex::new(None));
        let entered = Arc::new(tokio::sync::Notify::new());
        let mut providers = crate::core::ProviderBuilder::new();
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
        let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping real provider pool cleanup: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(());
        };
        let backend = DatabaseBackend::from_url(&url)?;
        let config = DatabaseInstanceConfig {
            min_connections: 1,
            connect_timeout_secs: 2,
            ..DatabaseInstanceConfig::default()
        };
        let owner = crate::core::resources::ResourceOwner::new();
        let registrar = owner.registrar();
        let first = connect_database(
            &url,
            &config,
            backend,
            "provider cleanup first pool",
            Some(&registrar),
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
    impl crate::core::ServiceProvider for Overlay {
        fn boot<'a>(
            &'a self,
            app: &'a mut crate::core::BootContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("database", json!({"table_prefix": "new_"})) })
        }
    }

    #[tokio::test]
    async fn missing_database_is_optional_and_overlay_configures_shared_handles() -> Result<()> {
        let mut builder = crate::core::ProviderBuilder::new();
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
    impl crate::core::ServiceProvider for LateReadiness {
        fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
            let calls = Arc::clone(&self.0);
            app.singleton::<crate::core::Readiness, _>(move |_| {
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(crate::core::Readiness::default()))
                })
            });
            Ok(())
        }
    }

    #[tokio::test]
    async fn boot_resolves_readiness_registered_by_later_provider() -> Result<()> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = crate::core::ProviderBuilder::new();
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
    impl crate::core::ServiceProvider for BrokenReadiness {
        fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
            app.singleton::<crate::core::Readiness, _>(|resolver| {
                Box::pin(async move {
                    resolver.resolve::<u128>().await?;
                    Ok(Arc::new(crate::core::Readiness::default()))
                })
            });
            Ok(())
        }
    }

    #[tokio::test]
    async fn bound_readiness_factory_failure_is_not_treated_as_optional() {
        let mut builder = crate::core::ProviderBuilder::new();
        builder
            .provider(DatabaseServiceProvider)
            .provider(BrokenReadiness);
        let error = builder.start(Figment::new()).await.err();
        assert!(error.is_some(), "bound readiness factory must fail startup");
    }

    #[cfg(feature = "db-postgres")]
    struct MalformedLateDatabaseUrl;
    #[cfg(feature = "db-postgres")]
    impl crate::core::ServiceProvider for MalformedLateDatabaseUrl {
        fn boot<'a>(
            &'a self,
            app: &'a mut crate::core::BootContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay("database", json!({"url": "postgres://[secret-sentinel"}))
            })
        }
    }

    #[cfg(feature = "db-postgres")]
    #[tokio::test]
    async fn malformed_late_database_url_fails_final_validation_without_leaking_secret() {
        let mut builder = crate::core::ProviderBuilder::new();
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
            let mut builder = crate::core::ProviderBuilder::new();
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
        assert!(DatabaseBackend::from_url("sqlite::memory:").is_err());
    }

    #[test]
    fn provisional_minimum_database_versions_are_checked_explicitly() {
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
