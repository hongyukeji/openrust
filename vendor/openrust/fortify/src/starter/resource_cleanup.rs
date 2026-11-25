//! 用户资源清理：账本与原生数据库队列同事务入库，Worker幂等删除独占文件。
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::{config::Repository, queue::JobHandler},
    database::{Connection, DatabaseManager, Row, Transaction, Value, eloquent::Model},
    filesystem::FilesystemManager,
    queue::{
        DispatchOptions, DispatchResult, JobContext, Payload, QueueError, QueueFuture,
        QueueManager, QueueResult,
    },
    support::{Str, facades::Date},
};
use serde::{Deserialize, Serialize};
use std::{
    any::{TypeId, type_name},
    sync::Arc,
};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;
const TABLE: &str = "user_resource_cleanups";
const JOB: &str = "openrust.user.cleanup";

/// 只接受套件随机生成的独占头像路径；不能将任意业务文件放入该清理队列。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredResource {
    pub disk: String,
    pub path: String,
}
impl StoredResource {
    pub fn validate(&self) -> AuthResult<()> {
        let name = self
            .path
            .strip_prefix("profile-photos/")
            .ok_or(AuthError::Configuration("用户清理文件不属于套件头像目录"))?;
        let Some((stem, extension)) = name.rsplit_once('.') else {
            return Err(AuthError::Configuration("用户清理文件缺少扩展名"));
        };
        if self.disk.is_empty()
            || self.disk.len() > 255
            || self.disk.trim() != self.disk
            || self.disk.chars().any(char::is_control)
            || !(40..=64).contains(&stem.len())
            || !stem.bytes().all(|b| b.is_ascii_alphanumeric())
            || !matches!(extension, "jpg" | "jpeg" | "png" | "webp")
        {
            return Err(AuthError::Configuration("用户清理文件路径不合法"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupKind {
    Avatar,
    AccountDeletion,
}
/// 收据明文仅交付一次；数据库和队列不保存它，状态查询不依赖已删除用户。
pub struct CleanupTicket {
    pub id: String,
    pub receipt: Zeroizing<String>,
    pub expires_at: i64,
}
#[derive(Serialize)]
pub struct CleanupStatus {
    pub id: String,
    pub status: String,
    pub total_files: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobData {
    operation: String,
    connection: String,
    scope: String,
}

pub struct ResourceCleanup {
    config: Arc<dyn Repository>,
    database: Arc<DatabaseManager>,
    filesystems: Arc<FilesystemManager>,
    model: TypeId,
    model_name: &'static str,
    table: String,
}
fn backend(error: impl std::error::Error + Send + Sync + 'static) -> AuthError {
    AuthError::Backend(Box::new(error))
}
fn now() -> AuthResult<i64> {
    Ok(Date::now(None)
        .map_err(backend)?
        .as_date_time()
        .timestamp()
        .as_second())
}
fn hash(bytes: &[u8]) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn text<'a>(row: &'a Row, key: &str) -> AuthResult<&'a str> {
    match row.get(key) {
        Some(Value::Text(v)) => Ok(v),
        _ => Err(AuthError::Configuration("用户资源清理记录损坏")),
    }
}
fn number(row: &Row, key: &str) -> AuthResult<i64> {
    match row.get(key) {
        Some(Value::Integer(v)) => Ok(*v),
        Some(Value::Unsigned(v)) => i64::try_from(*v).map_err(backend),
        _ => Err(AuthError::Configuration("用户资源清理时间损坏")),
    }
}
impl ResourceCleanup {
    fn for_model<M: Model>(
        config: Arc<dyn Repository>,
        database: Arc<DatabaseManager>,
        filesystems: Arc<FilesystemManager>,
    ) -> Self {
        Self {
            config,
            database,
            filesystems,
            model: TypeId::of::<M>(),
            model_name: type_name::<M>(),
            table: M::from_state(Default::default()).get_table().into(),
        }
    }
    fn scope(&self, connection: &Connection) -> AuthResult<String> {
        Ok(hash(&serde_json::to_vec(&(
            "user-resource-cleanup:v1",
            self.model_name,
            &self.table,
            connection.get_name(),
            connection.get_database_name(),
            connection.get_table_prefix(),
            connection.get_config("search_path"),
        ))?))
    }
    fn option(&self, key: &str, default: &str) -> AuthResult<String> {
        match self.config.get(key) {
            None | Some(serde_json::Value::Null) => Ok(default.into()),
            Some(serde_json::Value::String(v))
                if !v.is_empty()
                    && v.len() <= 255
                    && v.trim() == v
                    && !v.chars().any(char::is_control) =>
            {
                Ok(v)
            }
            _ => Err(AuthError::Configuration("用户清理队列配置必须为非空字符串")),
        }
    }
    /// 必须在用户所属事务内调用；不支持同库原子入队时失败，由外层回滚全部更改。
    pub async fn enqueue<M: Model>(
        &self,
        user: &M,
        transaction: &mut Transaction,
        queue: &QueueManager,
        kind: CleanupKind,
        resources: Vec<StoredResource>,
    ) -> AuthResult<CleanupTicket> {
        if !queue.has_handler(JOB) {
            return Err(AuthError::Configuration("用户资源清理任务尚未装配"));
        }
        if TypeId::of::<M>() != self.model || user.get_table() != self.table || resources.len() > 16
        {
            return Err(AuthError::Configuration("用户资源清理模型或文件清单不匹配"));
        }
        for file in &resources {
            file.validate()?;
        }
        let connection = user.get_connection()?;
        if !transaction.belongs_to(&connection) {
            return Err(AuthError::Configuration(
                "用户资源清理必须使用本人数据库事务",
            ));
        }
        let scope = self.scope(&connection)?;
        let owner = AuthIdentifier::try_from(user.get_key().ok_or(AuthError::InvalidIdentity)?)?;
        let owner_hash = hash(&serde_json::to_vec(&(&scope, user.get_key_name(), owner))?);
        let receipt = Zeroizing::new(Str::random(64));
        let id = hash(Str::random(64).as_bytes());
        let created = now()?;
        let expires_at = created
            .checked_add(604800)
            .ok_or(AuthError::Configuration("用户清理收据时间溢出"))?;
        transaction
            .table(TABLE)
            .insert(&[[
                ("id".into(), id.clone().into()),
                ("scope".into(), scope.clone().into()),
                ("owner_hash".into(), owner_hash.into()),
                ("receipt_hash".into(), hash(receipt.as_bytes()).into()),
                (
                    "kind".into(),
                    match kind {
                        CleanupKind::Avatar => "avatar",
                        CleanupKind::AccountDeletion => "account-deletion",
                    }
                    .into(),
                ),
                (
                    "resources".into(),
                    serde_json::to_string(&resources)?.into(),
                ),
                ("status".into(), "pending".into()),
                ("created_at".into(), created.into()),
                ("updated_at".into(), created.into()),
                ("expires_at".into(), expires_at.into()),
            ]
            .into()])
            .await?;
        let payload = Payload::new(
            JOB,
            JobData {
                operation: id.clone(),
                connection: connection.get_name().into(),
                scope,
            },
        )
        .map_err(backend)?;
        let result = queue
            .dispatch_in_database_transaction(
                payload,
                DispatchOptions {
                    connection: Some(self.option("user.cleanup.connection", "database")?),
                    queue: Some(self.option("user.cleanup.queue", "user-resources")?),
                    ..Default::default()
                },
                transaction,
            )
            .await
            .map_err(backend)?;
        if !matches!(result, DispatchResult::Queued(_)) {
            return Err(AuthError::Configuration("用户资源清理未写入持久化队列"));
        }
        Ok(CleanupTicket {
            id,
            receipt,
            expires_at,
        })
    }
    /// 只返回有限状态，不返回用户资料、内部文件路径或异常详情。
    pub async fn status(
        &self,
        connection: &Connection,
        id: &str,
        receipt: &str,
    ) -> AuthResult<Option<CleanupStatus>> {
        if id.len() != 64
            || !id
                .bytes()
                .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
            || receipt.len() != 64
            || !receipt.bytes().all(|v| v.is_ascii_alphanumeric())
        {
            return Ok(None);
        }
        let Some(row) = connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", id)
            .first()
            .await?
        else {
            return Ok(None);
        };
        if !bool::from(
            text(&row, "receipt_hash")?
                .as_bytes()
                .ct_eq(hash(receipt.as_bytes()).as_bytes()),
        ) || number(&row, "expires_at")? <= now()?
            || text(&row, "scope")? != self.scope(connection)?
        {
            return Ok(None);
        }
        let resources: Vec<StoredResource> = serde_json::from_str(text(&row, "resources")?)?;
        let status = text(&row, "status")?;
        if !matches!(status, "pending" | "failed" | "completed") {
            return Err(AuthError::Configuration("用户资源清理状态损坏"));
        }
        Ok(Some(CleanupStatus {
            id: id.into(),
            status: status.into(),
            total_files: resources.len(),
        }))
    }
    /// 仅清理收据已过期且文件清理完成的记录；失败和待处理任务继续保留。
    pub async fn prune_completed(&self, database: Option<&str>) -> AuthResult<u64> {
        let name = if let Some(name) = database {
            Some(name.to_owned())
        } else {
            let queue = self.option("user.cleanup.connection", "database")?;
            let config = self
                .config
                .get("queue.connections")
                .and_then(|v| v.get(&queue).cloned())
                .ok_or(AuthError::Configuration(
                    "用户清理队列连接不存在，请显式指定database",
                ))?;
            if config.get("driver").and_then(serde_json::Value::as_str) != Some("database") {
                return Err(AuthError::Configuration("用户清理记录须指定数据库连接"));
            }
            match config.get("connection") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(v)) if !v.is_empty() => Some(v.clone()),
                _ => return Err(AuthError::Configuration("用户清理数据库连接配置错误")),
            }
        };
        let connection = self.database.connection(name.as_deref())?;
        Ok(connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("scope", "=", self.scope(&connection)?)
            .r#where("status", "=", "completed")
            .r#where("expires_at", "<=", now()?)
            .delete()
            .await?)
    }
    async fn run(&self, payload: &Payload) -> AuthResult<()> {
        let job: JobData = serde_json::from_value(payload.data().clone())?;
        let connection = self.database.connection(Some(&job.connection))?;
        if job.scope != self.scope(&connection)? {
            return Err(AuthError::Configuration(
                "用户资源清理Worker的数据库或模型范围已变化",
            ));
        }
        let row = connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", job.operation.clone())
            .first()
            .await?
            .ok_or(AuthError::Configuration("用户资源清理记录不存在"))?;
        if text(&row, "scope")? != job.scope {
            return Err(AuthError::Configuration("用户资源清理任务范围不匹配"));
        }
        match text(&row, "status")? {
            "completed" => return Ok(()),
            "pending" | "failed" => {}
            _ => return Err(AuthError::Configuration("用户资源清理状态损坏")),
        }
        let resources: Vec<StoredResource> = serde_json::from_str(text(&row, "resources")?)?;
        if resources.len() > 16 {
            return Err(AuthError::Configuration("用户资源清理记录过大"));
        }
        for file in resources {
            file.validate()?;
            let disk = self.filesystems.disk(Some(&file.disk)).map_err(backend)?;
            if !disk.delete(&file.path).await.map_err(backend)?
                && disk.exists(&file.path).await.map_err(backend)?
            {
                return Err(AuthError::Configuration("用户资源文件尚未删除，等待重试"));
            }
        }
        connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", job.operation)
            .update(
                [
                    ("status".into(), "completed".into()),
                    ("updated_at".into(), now()?.into()),
                ]
                .into(),
            )
            .await?;
        Ok(())
    }
    async fn mark_failed(&self, payload: &Payload) -> AuthResult<()> {
        let job: JobData = serde_json::from_value(payload.data().clone())?;
        let connection = self.database.connection(Some(&job.connection))?;
        if job.scope != self.scope(&connection)? {
            return Err(AuthError::Configuration("用户资源清理失败回调范围不匹配"));
        }
        connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", job.operation)
            .r#where("scope", "=", job.scope)
            .r#where("status", "!=", "completed")
            .update(
                [
                    ("status".into(), "failed".into()),
                    ("updated_at".into(), now()?.into()),
                ]
                .into(),
            )
            .await?;
        Ok(())
    }
}
struct Handler(Arc<ResourceCleanup>);
impl JobHandler for Handler {
    fn handle<'a>(
        &'a self,
        _context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move { self.0.run(payload).await.map_err(QueueError::execution) })
    }
    fn failed<'a>(
        &'a self,
        _context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        _error: &'a QueueError,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            self.0
                .mark_failed(payload)
                .await
                .map_err(QueueError::execution)
        })
    }
}
struct Registration(TypeId);
pub(super) fn configure<M: Model>(
    app: &openrust::foundation::Application,
) -> openrust::support::ProviderResult {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let container = app.container();
    if container.has(openrust::container::ServiceKey::typed::<Registration>()) {
        if container.make::<Registration>()?.0 != TypeId::of::<M>() {
            return Err(AuthError::Configuration("用户资源清理已绑定其他模型").into());
        }
        return Ok(());
    }
    let cleanup = Arc::new(ResourceCleanup::for_model::<M>(
        container.make::<dyn Repository>()?,
        container.make::<DatabaseManager>()?,
        container.make::<FilesystemManager>()?,
    ));
    let queue = container.make::<QueueManager>()?;
    if queue.has_handler(JOB) {
        return Err(AuthError::Configuration("用户资源清理任务名称已被其他处理器占用").into());
    }
    queue.register(JOB, Arc::new(Handler(cleanup.clone())))?;
    container.instance(cleanup)?;
    container.instance(Arc::new(Registration(TypeId::of::<M>())))?;
    use openrust::support::ServiceProvider as _;
    super::StarterKitServiceProvider.commands(app, crate::console::prune_user_cleanups::register);
    Ok(())
}
