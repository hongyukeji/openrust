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
    time::Duration,
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
/// 提交预留事务后才能写文件；仅由服务构造，不能从客户端数据反序列化。
pub struct UploadReservation {
    operation: String,
    connection: String,
    scope: String,
    owner_hash: String,
    resource: StoredResource,
}
impl UploadReservation {
    pub fn id(&self) -> &str {
        &self.operation
    }
    pub fn resource(&self) -> &StoredResource {
        &self.resource
    }
}
const UPLOAD_GRACE: i64 = 900;

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
        let kind = match kind {
            CleanupKind::Avatar => "avatar",
            CleanupKind::AccountDeletion => "account-deletion",
        };
        self.enqueue_operation(user, transaction, queue, kind, resources, None)
            .await
    }
    async fn enqueue_operation<M: Model>(
        &self,
        user: &M,
        transaction: &mut Transaction,
        queue: &QueueManager,
        kind: &str,
        resources: Vec<StoredResource>,
        delay: Option<openrust::queue::Delay>,
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
                ("kind".into(), kind.into()),
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
                    delay,
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
    /// 在独立用户事务中预留新头像回收任务；正常附着和回收通过同一记录互斥。
    pub async fn reserve_upload<M: Model>(
        &self,
        user: &M,
        transaction: &mut Transaction,
        queue: &QueueManager,
        resource: StoredResource,
    ) -> AuthResult<UploadReservation> {
        let connection = user.get_connection()?;
        let scope = self.scope(&connection)?;
        let owner = AuthIdentifier::try_from(user.get_key().ok_or(AuthError::InvalidIdentity)?)?;
        let owner_hash = hash(&serde_json::to_vec(&(&scope, user.get_key_name(), owner))?);
        let ticket = self
            .enqueue_operation(
                user,
                transaction,
                queue,
                "avatar-upload",
                vec![resource.clone()],
                Some(openrust::queue::Delay::For(Duration::from_secs(
                    UPLOAD_GRACE as u64,
                ))),
            )
            .await?;
        Ok(UploadReservation {
            operation: ticket.id,
            connection: connection.get_name().into(),
            scope,
            owner_hash,
            resource,
        })
    }
    /// 必须与头像引用更新一起提交；已被回收或到期的预留不能再次附着。
    pub async fn retain_upload<M: Model>(
        &self,
        user: &M,
        transaction: &mut Transaction,
        reservation: &UploadReservation,
    ) -> AuthResult<()> {
        let connection = user.get_connection()?;
        let scope = self.scope(&connection)?;
        let owner = AuthIdentifier::try_from(user.get_key().ok_or(AuthError::InvalidIdentity)?)?;
        let owner_hash = hash(&serde_json::to_vec(&(&scope, user.get_key_name(), owner))?);
        if TypeId::of::<M>() != self.model
            || user.get_table() != self.table
            || !transaction.belongs_to(&connection)
            || reservation.scope != scope
            || reservation.owner_hash != owner_hash
            || reservation.connection != connection.get_name()
        {
            return Err(AuthError::Configuration("头像上传预留不属于当前用户事务"));
        }
        let cutoff = now()?
            .checked_sub(UPLOAD_GRACE)
            .ok_or(AuthError::Configuration("头像上传预留时间溢出"))?;
        let changed = transaction
            .table(TABLE)
            .r#where("id", "=", reservation.operation.clone())
            .r#where("scope", "=", scope)
            .r#where("owner_hash", "=", owner_hash)
            .r#where("kind", "=", "avatar-upload")
            .r#where("status", "=", "pending")
            .r#where("created_at", ">", cutoff)
            .update(
                [
                    ("status".into(), "completed".into()),
                    ("updated_at".into(), now()?.into()),
                ]
                .into(),
            )
            .await?;
        if changed != 1 {
            return Err(AuthError::Configuration(
                "头像上传预留已过期或正在回收，请重新上传",
            ));
        }
        Ok(())
    }
    /// 请求失败时尽快补偿；已附着的新头像不会被取消，延迟任务仍可处理进程中断。
    pub async fn cancel_upload(&self, reservation: &UploadReservation) -> AuthResult<()> {
        let connection = self.database.connection(Some(&reservation.connection))?;
        if reservation.scope != self.scope(&connection)? {
            return Err(AuthError::Configuration("头像上传取消范围不匹配"));
        }
        connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", reservation.operation.clone())
            .r#where("scope", "=", reservation.scope.clone())
            .r#where("owner_hash", "=", reservation.owner_hash.clone())
            .r#where("kind", "=", "avatar-upload")
            .r#where("status", "=", "pending")
            .update(
                [
                    ("status".into(), "processing".into()),
                    ("updated_at".into(), now()?.into()),
                ]
                .into(),
            )
            .await?;
        let payload = Payload::new(
            JOB,
            JobData {
                operation: reservation.operation.clone(),
                connection: reservation.connection.clone(),
                scope: reservation.scope.clone(),
            },
        )
        .map_err(backend)?;
        // 完成记录表示已附着或已清理，run会保持幂等，不删除当前引用。
        self.run(&payload).await?;
        Ok(())
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
        if !matches!(status, "pending" | "processing" | "failed" | "completed") {
            return Err(AuthError::Configuration("用户资源清理状态损坏"));
        }
        Ok(Some(CleanupStatus {
            id: id.into(),
            status: if status == "processing" {
                "pending".into()
            } else {
                status.into()
            },
            total_files: resources.len(),
        }))
    }
    fn configured_database(&self, database: Option<&str>) -> AuthResult<Connection> {
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
        Ok(self.database.connection(name.as_deref())?)
    }
    pub async fn status_for_receipt(
        &self,
        id: &str,
        receipt: &str,
    ) -> AuthResult<Option<CleanupStatus>> {
        self.status(&self.configured_database(None)?, id, receipt)
            .await
    }
    /// 仅清理收据已过期且文件清理完成的记录；失败和待处理任务继续保留。
    pub async fn prune_completed(&self, database: Option<&str>) -> AuthResult<u64> {
        let connection = self.configured_database(database)?;
        Ok(connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("scope", "=", self.scope(&connection)?)
            .r#where("status", "=", "completed")
            .r#where("expires_at", "<=", now()?)
            .delete()
            .await?)
    }
    async fn run(&self, payload: &Payload) -> AuthResult<Option<Duration>> {
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
        let status = text(&row, "status")?;
        match status {
            "completed" => return Ok(None),
            "pending" | "processing" | "failed" => {}
            _ => return Err(AuthError::Configuration("用户资源清理状态损坏")),
        }
        match text(&row, "kind")? {
            "avatar-upload" if status == "pending" => {
                let deadline = number(&row, "created_at")?
                    .checked_add(UPLOAD_GRACE)
                    .ok_or(AuthError::Configuration("头像上传预留时间溢出"))?;
                let current = now()?;
                if deadline > current {
                    return Ok(Some(Duration::from_secs((deadline - current) as u64)));
                }
                let claimed = connection
                    .table(TABLE)
                    .use_write_pdo()
                    .r#where("id", "=", job.operation.clone())
                    .r#where("scope", "=", job.scope.clone())
                    .r#where("status", "=", "pending")
                    .update(
                        [
                            ("status".into(), "processing".into()),
                            ("updated_at".into(), current.into()),
                        ]
                        .into(),
                    )
                    .await?;
                if claimed == 0 {
                    let latest = connection
                        .table(TABLE)
                        .use_write_pdo()
                        .r#where("id", "=", job.operation.clone())
                        .first()
                        .await?
                        .ok_or(AuthError::Configuration("头像上传预留记录已移除"))?;
                    match text(&latest, "status")? {
                        "completed" => return Ok(None),
                        "processing" | "failed" => {}
                        _ => return Err(AuthError::Configuration("头像上传预留未取得回收资格")),
                    }
                }
            }
            "avatar-upload" => {}
            "avatar" | "account-deletion" if status != "processing" => {}
            _ => return Err(AuthError::Configuration("用户资源清理类型损坏")),
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
        if text(&row, "kind")? == "avatar-upload" {
            let deadline = number(&row, "created_at")?
                .checked_add(UPLOAD_GRACE)
                .ok_or(AuthError::Configuration("头像上传预留时间溢出"))?;
            let current = now()?;
            if deadline > current {
                return Ok(Some(Duration::from_secs((deadline - current) as u64)));
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
        Ok(None)
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
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            if let Some(delay) = self.0.run(payload).await.map_err(QueueError::execution)? {
                context.release(delay);
            }
            Ok(())
        })
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
    let limiter = container.make::<openrust::cache::RateLimiter>()?;
    if limiter.limiter("user-resource-status").is_none()
        && limiter.async_limiter("user-resource-status").is_none()
    {
        limiter.r#for("user-resource-status", |request| {
            openrust::cache::rate_limiting::Limit::per_minute(60).by(request
                .ip()
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "unknown".into()))
        });
    }

    container.instance(cleanup)?;
    container.instance(Arc::new(Registration(TypeId::of::<M>())))?;
    use openrust::support::ServiceProvider as _;
    super::StarterKitServiceProvider.commands(app, crate::console::prune_user_cleanups::register);
    super::StarterKitServiceProvider.load_routes_from(app, status_routes)?;
    Ok(())
}

fn status_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::{
        routing::RouteGroup,
        support::facades::{Config, Route},
    };
    let prefix = Config::string_with_default("api.prefix", "api")
        .map_err(|_| openrust::routing::RoutingError::InvalidAction)?;
    // 收据是有限状态能力，不进入有Session/认证副作用的api组。
    Route::group(
        RouteGroup::new()
            .prefix(prefix)
            .middleware(Arc::new(super::client_middleware::NoStore)),
        |router| {
            router
                .get("/auth/resource-cleanups/{operation}", show_status)?
                .middleware_name("throttle:user-resource-status")
                .name("user.resource-cleanups.show");
            Ok(())
        },
    )
}
fn show_status(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{
            http::{ApiResponse, HttpError},
            support::facades::App,
        };
        if request.query(Some("receipt"))?.is_some() {
            return Err(HttpError::InvalidRequest);
        }
        let id = request.route("operation").ok_or(HttpError::NotFound)?;
        let receipt = request
            .header("x-operation-receipt")?
            .ok_or(HttpError::NotFound)?;
        let cleanup = App::make::<ResourceCleanup>().await?;
        let status = cleanup
            .status_for_receipt(id, receipt)
            .await?
            .ok_or(HttpError::NotFound)?;
        Ok(
            super::client::no_store(ApiResponse::success(status).into_response()?).header(
                "referrer-policy".parse().expect("静态头名称"),
                "no-referrer".parse().expect("静态头值"),
            ),
        )
    })
}
