//! 文件存储、流式对象访问与公开 driver 扩展。
#![doc = include_str!("filesystem/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{
    collections::BTreeMap,
    fmt,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::de::DeserializeOwned;

use crate::core::{Error, Result};
use crate::support::BoxFuture;

pub use crate::config::{StorageConfig, StorageDiskConfig};

/// 存储读取或写入所使用的有序 bytes stream。
pub type StorageStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send + 'static>>;

/// 对象或本地文件的可见性；public 不保证网络可达。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// 由存储服务的公开读取策略访问。
    Public,
    /// 仅由授权调用方访问。
    #[default]
    Private,
}

/// 已验证的相对对象 key。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoragePath(String);

impl StoragePath {
    /// 校验并创建 storage path。
    pub fn new(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        if path.is_empty()
            || path.len() > 1024
            || path.starts_with('/')
            || path.contains('\\')
            || path.starts_with(".staging/")
            || path == ".staging"
            || path.starts_with(".openrust-staging/")
            || path == ".openrust-staging"
            || path.bytes().any(|byte| byte.is_ascii_control())
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(Error::custom(
                "storage_invalid_path",
                "storage object key must be a normalized relative path",
            ));
        }
        Ok(Self(path))
    }

    /// 返回规范化后的对象 key。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 存储对象的可用元数据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageMetadata {
    /// 对象字节数。
    pub size: u64,
    /// 存储后端返回的媒体类型。
    pub content_type: Option<String>,
    /// 存储后端返回的 `ETag`。
    pub etag: Option<String>,
    /// 存储后端返回的对象版本。
    pub version: Option<String>,
}

/// 一次上传的可验证内容声明。
#[derive(Debug, Clone)]
pub struct UploadContext {
    /// 目标对象 key。
    pub path: StoragePath,
    /// 请求声明的媒体类型；它不等同于内容嗅探结果。
    pub content_type: Option<String>,
}

/// 上传期间按顺序检查每个数据块的应用扩展点。
#[async_trait]
pub trait UploadInspector: Send + Sync + 'static {
    /// 检查当前数据块；返回错误会在目标对象发布前中止写入。
    async fn inspect_chunk(&self, context: &UploadContext, chunk: &[u8]) -> Result<()>;

    /// 通知 inspector 输入已经完整到达 EOF。
    async fn finish(&self, _context: &UploadContext, _total_bytes: u64) -> Result<()> {
        Ok(())
    }
}

/// 约束总上传字节数、单个块的大小和声明的媒体类型。
#[derive(Debug, Clone)]
pub struct UploadOptions {
    /// 覆盖 disk 默认可见性；未指定时使用 disk 配置。
    pub visibility: Option<Visibility>,
    /// 此次上传允许的最大总字节数。
    pub max_bytes: u64,
    /// 此次上传允许的最大单块字节数。
    pub max_chunk_bytes: usize,
    /// 可选的 MIME allowlist；为空时不筛选声明值。
    pub allowed_content_types: Vec<String>,
}

impl Default for UploadOptions {
    fn default() -> Self {
        Self {
            visibility: None,
            max_bytes: 32 * 1024 * 1024,
            max_chunk_bytes: 1024 * 1024,
            allowed_content_types: Vec::new(),
        }
    }
}

/// 已完成或已读取对象的 metadata。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    /// 对象 key。
    pub path: StoragePath,
    /// 后端返回的 metadata。
    pub metadata: StorageMetadata,
}

/// 流式读取结果；stream 消费期间发生的错误由每个 chunk 单独返回。
pub struct StorageReader {
    /// 打开时观察到的对象 metadata。
    pub metadata: StorageMetadata,
    /// 按顺序读取对象数据的 stream。
    pub body: StorageStream,
}

/// 应用可实现的文件存储 driver 契约。
#[async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    /// 写入 stream；提交目标前须通过 `UploadCancellation::begin_publication`
    /// 领取发布权，成功或已知失败时结清令牌。失败/取消不能发布部分对象。
    async fn put(
        &self,
        path: &StoragePath,
        content_type: Option<&str>,
        visibility: Option<Visibility>,
        body: StorageStream,
        cancellation: UploadCancellation,
    ) -> Result<StorageMetadata>;

    /// 读取对象的实际可见性；不支持时明确失败。
    async fn visibility(&self, _path: &StoragePath) -> Result<Visibility> {
        Err(unsupported_storage_capability("visibility reads"))
    }

    /// 修改对象可见性；不支持时明确失败。
    async fn set_visibility(&self, _path: &StoragePath, _visibility: Visibility) -> Result<()> {
        Err(unsupported_storage_capability("visibility changes"))
    }

    /// 打开对象的有序 stream；可选 version 必须精确绑定对象版本。
    async fn open(&self, path: &StoragePath, version: Option<&str>) -> Result<StorageReader>;

    /// 读取对象 metadata。
    async fn stat(&self, path: &StoragePath) -> Result<StorageMetadata>;

    /// 删除对象。
    async fn delete(&self, path: &StoragePath) -> Result<()>;

    /// 按具体对象版本创建 presigned read URL；不支持时返回明确错误。
    async fn presign_read(
        &self,
        _path: &StoragePath,
        _version: &str,
        _expires_in: Duration,
    ) -> Result<String> {
        Err(unsupported_storage_capability("presigned read"))
    }
}

/// 上传调用在发布仲裁前被丢弃时通知 driver 清理未发布数据。
pub struct UploadCancellation {
    receiver: tokio::sync::oneshot::Receiver<()>,
    publication: Arc<AtomicU8>,
}

const UPLOAD_PENDING: u8 = 0;
const UPLOAD_CANCELLED: u8 = 1;
const UPLOAD_PUBLISHING: u8 = 2;
const UPLOAD_FINISHED: u8 = 3;
const UPLOAD_FAILED: u8 = 4;
const UPLOAD_UNCERTAIN: u8 = 5;

impl UploadCancellation {
    /// 等待调用方取消；正常 EOF 不会触发该通知。
    pub async fn cancelled(&mut self) {
        let _ = (&mut self.receiver).await;
    }

    /// 领取最终发布权；取消先赢时目标不得提交。领取后 driver 须完成原子提交，
    /// 即使调用方 future 已被丢弃也须自行完成或报告结果不确定。
    pub fn begin_publication(&self) -> Result<UploadPublication> {
        self.publication
            .compare_exchange(
                UPLOAD_PENDING,
                UPLOAD_PUBLISHING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| UploadPublication {
                state: Arc::clone(&self.publication),
            })
            .map_err(|state| match state {
                UPLOAD_CANCELLED => Error::custom(
                    "storage_upload_cancelled",
                    "upload was cancelled before publication",
                ),
                _ => Error::internal(std::io::Error::other(
                    "upload publication was already claimed",
                )),
            })
    }
}

/// 已领取的最终发布权。只在目标完整提交后标记成功；已知失败时标记失败。
#[must_use = "publication must be completed or failed after the final commit attempt"]
pub struct UploadPublication {
    state: Arc<AtomicU8>,
}

impl UploadPublication {
    /// 确认完整对象已提交；driver 此后才能返回成功。
    pub fn complete(self) {
        self.state.store(UPLOAD_FINISHED, Ordering::Release);
    }

    /// 确认最终提交失败；已发送但响应丢失时应返回结果不确定错误。
    pub fn fail(self) {
        self.state.store(UPLOAD_FAILED, Ordering::Release);
    }

    /// 最终提交已发出但结果未知；driver 须返回 `storage_publication_uncertain`。
    pub fn uncertain(self) {
        self.state.store(UPLOAD_UNCERTAIN, Ordering::Release);
    }
}

struct UploadCancellationGuard {
    sender: Option<tokio::sync::oneshot::Sender<()>>,
    publication: Arc<AtomicU8>,
}

impl UploadCancellationGuard {
    fn cancel_pending(&mut self) -> bool {
        if self
            .publication
            .compare_exchange(
                UPLOAD_PENDING,
                UPLOAD_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            if let Some(sender) = self.sender.take() {
                let _ = sender.send(());
            }
            true
        } else {
            false
        }
    }

    fn complete(&mut self) -> Result<()> {
        let cancelled_here = self.cancel_pending();
        let _ = self.sender.take();
        match self.publication.load(Ordering::Acquire) {
            UPLOAD_FINISHED => Ok(()),
            UPLOAD_CANCELLED if cancelled_here => Err(Error::custom(
                "storage_publication_incomplete",
                "driver returned success without claiming publication",
            )),
            UPLOAD_CANCELLED | UPLOAD_PUBLISHING | UPLOAD_UNCERTAIN => Err(Error::custom(
                "storage_publication_uncertain",
                "driver reported success without a confirmed publication outcome",
            )),
            _ => Err(Error::custom(
                "storage_publication_incomplete",
                "driver returned success without completing publication",
            )),
        }
    }
}

impl Drop for UploadCancellationGuard {
    fn drop(&mut self) {
        let _ = self.cancel_pending();
    }
}

/// 可从命名配置构造一个公开 storage backend。
pub trait StorageDriver: Send + Sync + 'static {
    /// driver 配置使用的稳定名称。
    fn name(&self) -> &'static str;

    /// 不执行 I/O 地校验 driver 配置；第三方 driver 可覆盖此方法。
    fn validate(&self, _config: &StorageDriverConfig) -> Result<()> {
        Ok(())
    }
    /// 初始化 driver；连接或配置错误会使应用启动失败。
    fn build<'a>(
        &'a self,
        config: &'a StorageDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>>;
}

/// 已解析的单个 disk 配置。
#[derive(Clone)]
pub struct StorageDriverConfig {
    disk_name: String,
    options: BTreeMap<String, serde_json::Value>,
}

impl StorageDriverConfig {
    /// 返回 disk 名称。
    pub fn disk_name(&self) -> &str {
        &self.disk_name
    }

    /// 按类型读取 driver 专属配置；错误中不会包含原始配置值。
    pub fn option<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.options
            .get(key)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| {
                Error::custom(
                    "storage_invalid_driver_option",
                    format!(
                        "storage disk `{}` has an invalid `{key}` option",
                        self.disk_name
                    ),
                )
            })
    }

    /// 拒绝未识别的 driver 专属配置键，避免拼写错误静默失效。
    pub fn reject_unknown_options(&self, allowed: &[&str]) -> Result<()> {
        if let Some(key) = self
            .options
            .keys()
            .find(|key| !allowed.contains(&key.as_str()))
        {
            return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
                "storage disk `{}` has an unknown `{key}` option",
                self.disk_name
            ))));
        }
        Ok(())
    }
}

impl fmt::Debug for StorageDriverConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StorageDriverConfig")
            .field("disk_name", &self.disk_name)
            .field("option_names", &self.options.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// 应用插件收集的具名 storage driver。
#[derive(Clone, Default)]
pub struct StorageDrivers {
    drivers: BTreeMap<&'static str, Arc<dyn StorageDriver>>,
}

impl StorageDrivers {
    /// 注册 driver；重复名称返回错误，不覆盖已有实现。
    pub fn register(&mut self, driver: impl StorageDriver) -> Result<()> {
        self.register_shared(Arc::new(driver))
    }

    fn register_shared(&mut self, driver: Arc<dyn StorageDriver>) -> Result<()> {
        let name = driver.name();
        if !valid_driver_name(name) {
            return Err(Error::custom(
                "storage_invalid_driver_name",
                "storage driver name must be lowercase ASCII",
            ));
        }
        if self.drivers.contains_key(name) {
            return Err(Error::custom(
                "storage_duplicate_driver",
                format!("storage driver `{name}` is already registered"),
            ));
        }
        self.drivers.insert(name, driver);
        Ok(())
    }
}

struct StorageManager {
    resolver: crate::container::Resolver,
    drivers: std::sync::Mutex<StorageDrivers>,
    disks: std::sync::Mutex<BTreeMap<String, Arc<tokio::sync::OnceCell<FilesystemAdapter>>>>,
}

/// 应用可查询 default 与 named storage disk 的统一服务。
#[derive(Clone)]
pub struct FilesystemManager {
    manager: Arc<StorageManager>,
}

impl FilesystemManager {
    fn managed(resolver: crate::container::Resolver, drivers: StorageDrivers) -> Self {
        Self {
            manager: Arc::new(StorageManager {
                resolver,
                drivers: std::sync::Mutex::new(drivers),
                disks: std::sync::Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// 注册或替换 driver creator；已解析的 disk 保持不变。
    pub fn extend(&self, driver: impl StorageDriver) -> Result<()> {
        let manager = &self.manager;
        let name = driver.name();
        if !valid_driver_name(name) {
            return Err(Error::custom(
                "storage_invalid_driver_name",
                "storage driver name must be lowercase ASCII",
            ));
        }
        let old = manager
            .drivers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drivers
            .insert(name, Arc::new(driver));
        drop(old);
        Ok(())
    }

    /// 移除已解析 disk，下一次读取时重新构造。
    pub fn forget_disk(&self, name: &str) -> Result<()> {
        let manager = &self.manager;
        let removed = manager
            .disks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(name);
        drop(removed);
        Ok(())
    }

    /// 移除指定 disk 的缓存引用。
    pub fn purge(&self, name: &str) -> Result<()> {
        self.forget_disk(name)
    }

    /// 校验当前配置与 driver 名称；driver 专属选项在构造时校验。
    pub fn validate_configured(&self) -> Result<()> {
        let manager = &self.manager;
        let config = manager.resolver.section::<StorageConfig>("storage")?;
        validate_storage_config(&config)?;
        let drivers = manager
            .drivers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for (name, disk) in config.disks {
            let driver = drivers.drivers.get(disk.driver.as_str()).ok_or_else(|| {
                Error::Config(crate::config::ConfigError::Invalid(
                    "unknown storage driver".into(),
                ))
            })?;
            driver.validate(&StorageDriverConfig {
                disk_name: name,
                options: disk.options,
            })?;
        }
        Ok(())
    }

    /// 返回当前配置选定的默认 disk。
    pub async fn default_disk(&self) -> Result<FilesystemAdapter> {
        let config = self.manager.resolver.section::<StorageConfig>("storage")?;
        self.disk(&config.default_disk).await
    }

    /// 按名称异步构造并缓存 disk；未知名称不会回退。
    pub async fn disk(&self, name: &str) -> Result<FilesystemAdapter> {
        let manager = &self.manager;
        validate_disk_name(name)?;
        let cell = manager
            .disks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name.to_owned())
            .or_default()
            .clone();
        let value = cell
            .get_or_try_init(|| async {
                let config = manager.resolver.section::<StorageConfig>("storage")?;
                validate_storage_config(&config)?;
                let disk = config.disks.get(name).ok_or_else(|| {
                    Error::custom("storage_disk_not_found", "storage disk is not configured")
                })?;
                let driver = manager
                    .drivers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .drivers
                    .get(disk.driver.as_str())
                    .cloned()
                    .ok_or_else(|| {
                        Error::Config(crate::config::ConfigError::Invalid(
                            "unknown storage driver".into(),
                        ))
                    })?;
                let driver_config = StorageDriverConfig {
                    disk_name: name.to_owned(),
                    options: disk.options.clone(),
                };
                driver.validate(&driver_config)?;
                let backend = driver.build(&driver_config).await?;
                Ok::<_, Error>(FilesystemAdapter {
                    name: name.to_owned(),
                    backend,
                    visibility: disk.visibility,
                    max_upload_bytes: config.max_upload_bytes,
                    max_chunk_bytes: config.max_chunk_bytes,
                })
            })
            .await?;
        Ok(value.clone())
    }
}

/// 已初始化的具名 storage disk。
#[derive(Clone)]
pub struct FilesystemAdapter {
    name: String,
    backend: Arc<dyn StorageBackend>,
    visibility: Option<Visibility>,
    max_upload_bytes: u64,
    max_chunk_bytes: usize,
}

impl FilesystemAdapter {
    /// 返回 disk 名称。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回 disk 显式配置的默认可见性；None 表示由 backend 决定。
    pub fn default_visibility(&self) -> Option<Visibility> {
        self.visibility
    }

    /// 读取对象实际可见性。
    pub async fn visibility(&self, path: &StoragePath) -> Result<Visibility> {
        self.backend.visibility(&tenant_storage_path(path)?).await
    }

    /// 设置对象实际可见性。
    pub async fn set_visibility(&self, path: &StoragePath, visibility: Visibility) -> Result<()> {
        self.backend
            .set_visibility(&tenant_storage_path(path)?, visibility)
            .await
    }

    /// 返回该 disk 的最大上传字节数。
    pub fn max_upload_bytes(&self) -> u64 {
        self.max_upload_bytes
    }

    /// 返回上传检查与 driver 接收的最大 chunk 大小。
    pub fn max_chunk_bytes(&self) -> usize {
        self.max_chunk_bytes
    }

    /// 将 HTTP request body 有界切块后流式写入对象。
    pub async fn put_body(
        &self,
        path: StoragePath,
        content_type: Option<String>,
        body: axum::body::Body,
        options: UploadOptions,
        inspector: Option<Arc<dyn UploadInspector>>,
    ) -> Result<StoredObject> {
        let body = http_body_stream(body, self.max_chunk_bytes);
        self.put_stream(path, content_type, body, options, inspector)
            .await
    }

    /// 流式写入对象；逐块执行上限、MIME allowlist 与可选 inspector 检查。
    pub async fn put_stream(
        &self,
        path: StoragePath,
        content_type: Option<String>,
        body: StorageStream,
        mut options: UploadOptions,
        inspector: Option<Arc<dyn UploadInspector>>,
    ) -> Result<StoredObject> {
        if options.max_bytes == 0 || options.max_chunk_bytes == 0 {
            return Err(Error::custom(
                "storage_invalid_upload_limit",
                "upload byte limits must be greater than zero",
            ));
        }
        options.max_bytes = options.max_bytes.min(self.max_upload_bytes);
        options.max_chunk_bytes = options.max_chunk_bytes.min(self.max_chunk_bytes);
        if let Some(content_type) = &content_type {
            validate_content_type(content_type)?;
        }
        for allowed_content_type in &options.allowed_content_types {
            validate_content_type(allowed_content_type)?;
        }
        if !options.allowed_content_types.is_empty()
            && !content_type.as_ref().is_some_and(|content_type| {
                options
                    .allowed_content_types
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(content_type))
            })
        {
            return Err(Error::custom(
                "storage_content_type_rejected",
                "upload content type is not allowed",
            ));
        }
        let context = UploadContext {
            path: path.clone(),
            content_type: content_type.clone(),
        };
        let visibility = options.visibility.or(self.visibility);
        let checked_body = checked_upload_stream(body, context.clone(), options, inspector);
        let physical_path = tenant_storage_path(&path)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let publication = Arc::new(AtomicU8::new(UPLOAD_PENDING));
        let mut cancellation_guard = UploadCancellationGuard {
            sender: Some(sender),
            publication: Arc::clone(&publication),
        };
        let result = self
            .backend
            .put(
                &physical_path,
                content_type.as_deref(),
                visibility,
                checked_body,
                UploadCancellation {
                    receiver,
                    publication: Arc::clone(&publication),
                },
            )
            .await;
        let mut metadata = match result {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = cancellation_guard.cancel_pending();
                return Err(match publication.load(Ordering::Acquire) {
                    UPLOAD_PUBLISHING | UPLOAD_UNCERTAIN | UPLOAD_FINISHED => Error::custom(
                        "storage_publication_uncertain",
                        "driver failed after publication began; target state must be checked",
                    ),
                    _ => error,
                });
            }
        };
        cancellation_guard.complete()?;
        if metadata.content_type.is_none() {
            metadata.content_type = content_type;
        }
        Ok(StoredObject { path, metadata })
    }

    /// 流式打开对象；可选的 version 必须由后端精确支持。
    pub async fn open(&self, path: &StoragePath, version: Option<&str>) -> Result<StorageReader> {
        let physical_path = tenant_storage_path(path)?;
        let mut reader = self.backend.open(&physical_path, version).await?;
        reader.body = bounded_storage_stream(reader.body, self.max_chunk_bytes);
        Ok(reader)
    }

    /// 读取对象 metadata。
    pub async fn stat(&self, path: &StoragePath) -> Result<StorageMetadata> {
        let physical_path = tenant_storage_path(path)?;
        self.backend.stat(&physical_path).await
    }

    /// 删除对象。
    pub async fn delete(&self, path: &StoragePath) -> Result<()> {
        let physical_path = tenant_storage_path(path)?;
        self.backend.delete(&physical_path).await
    }

    /// 生成绑定确切版本的 presigned read URL。
    pub async fn presign_read(&self, path: &StoragePath, expires_in: Duration) -> Result<String> {
        if expires_in.is_zero() || expires_in > Duration::from_hours(168) {
            return Err(Error::custom(
                "storage_invalid_presign_ttl",
                "presigned URL lifetime must be between one second and seven days",
            ));
        }
        let metadata = self.stat(path).await?;
        let version = metadata.version.as_deref().ok_or_else(|| {
            Error::custom(
                "storage_object_version_unavailable",
                "version-bound presigned reads require a concrete object version",
            )
        })?;
        let physical_path = tenant_storage_path(path)?;
        self.backend
            .presign_read(&physical_path, version, expires_in)
            .await
    }
}

fn tenant_storage_path(path: &StoragePath) -> Result<StoragePath> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let Some(tenant) = crate::tenant::TenantContext::current() else {
        return Ok(path.clone());
    };
    let digest = Sha256::digest(tenant.as_str().as_bytes());
    let mut prefix = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut prefix, "{byte:02x}").map_err(Error::internal)?;
    }
    Ok(StoragePath(format!("tenants/{prefix}/{}", path.as_str())))
}

/// 限制交给 inspector 和 driver 的块大小；不限制上游已分配的 Axum body frame。
fn http_body_stream(body: axum::body::Body, max_chunk_bytes: usize) -> StorageStream {
    let input = body.into_data_stream();
    let body = Box::pin(input.map(|chunk| chunk.map_err(Error::internal)));
    bounded_storage_stream(body, max_chunk_bytes)
}

fn bounded_storage_stream(body: StorageStream, max_chunk_bytes: usize) -> StorageStream {
    Box::pin(futures_util::stream::unfold(
        (body, None::<Bytes>),
        move |(mut body, mut pending)| async move {
            loop {
                if let Some(bytes) = pending.take() {
                    if bytes.len() > max_chunk_bytes {
                        let chunk = bytes.slice(..max_chunk_bytes);
                        pending = Some(bytes.slice(max_chunk_bytes..));
                        return Some((Ok(chunk), (body, pending)));
                    }
                    return Some((Ok(bytes), (body, None)));
                }
                match body.next().await {
                    Some(Ok(bytes)) => pending = Some(bytes),
                    Some(Err(error)) => return Some((Err(error), (body, None))),
                    None => return None,
                }
            }
        },
    ))
}

/// 按单块边界检查 stream，并在 EOF 时完成 inspector。
fn checked_upload_stream(
    body: StorageStream,
    context: UploadContext,
    options: UploadOptions,
    inspector: Option<Arc<dyn UploadInspector>>,
) -> StorageStream {
    let state = (body, 0_u64, false);
    Box::pin(futures_util::stream::unfold(
        state,
        move |(mut body, mut total, finished)| {
            let context = context.clone();
            let options = options.clone();
            let inspector = inspector.clone();
            async move {
                if finished {
                    return None;
                }
                match body.next().await {
                    Some(Ok(chunk)) => {
                        let error = if chunk.len() > options.max_chunk_bytes {
                            Some(Error::custom(
                                "storage_upload_chunk_too_large",
                                "upload stream chunk exceeds its configured limit",
                            ))
                        } else {
                            match total.checked_add(chunk.len() as u64) {
                                Some(next_total) if next_total <= options.max_bytes => {
                                    total = next_total;
                                    None
                                }
                                _ => Some(Error::custom(
                                    "storage_upload_too_large",
                                    "upload stream exceeds its configured byte limit",
                                )),
                            }
                        };
                        if let Some(error) = error {
                            return Some((Err(error), (body, total, true)));
                        }
                        if let Some(inspector) = inspector
                            && let Err(error) = inspector.inspect_chunk(&context, &chunk).await
                        {
                            return Some((Err(error), (body, total, true)));
                        }
                        Some((Ok(chunk), (body, total, false)))
                    }
                    Some(Err(error)) => Some((Err(error), (body, total, true))),
                    None => {
                        if let Some(inspector) = inspector
                            && let Err(error) = inspector.finish(&context, total).await
                        {
                            return Some((Err(error), (body, total, true)));
                        }
                        None
                    }
                }
            }
        },
    ))
}

/// 按当前配置装配内置及应用自定义存储 driver。
///
/// Provider 尚待生产入口统一切换。
#[derive(Default)]
pub struct FilesystemServiceProvider {
    drivers: Vec<Arc<dyn StorageDriver>>,
}

impl FilesystemServiceProvider {
    /// 创建不含应用自定义 driver 的 Provider。
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加应用 driver；与既有名称冲突时启动失败。
    #[must_use]
    pub fn with_driver(mut self, driver: impl StorageDriver) -> Self {
        self.drivers.push(Arc::new(driver));
        self
    }
}

fn validate_storage_config(config: &StorageConfig) -> Result<()> {
    validate_disk_name(&config.default_disk)?;
    if !config.disks.contains_key(&config.default_disk) {
        return Err(Error::Config(crate::config::ConfigError::Invalid(
            "storage.default_disk is not configured".into(),
        )));
    }
    if config.max_upload_bytes == 0
        || config.max_upload_bytes > 5 * 1024 * 1024 * 1024
        || config.max_chunk_bytes == 0
        || config.max_chunk_bytes > 8 * 1024 * 1024
    {
        return Err(Error::Config(crate::config::ConfigError::Invalid(
            "storage upload limits are outside their supported range".into(),
        )));
    }
    for name in config.disks.keys() {
        validate_disk_name(name)?;
    }
    Ok(())
}

impl crate::support::ServiceProvider for FilesystemServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as S;
            app.config_section(
                "storage",
                serde_json::json!({}),
                S::object([
                    ("default_disk", S::String),
                    ("max_upload_bytes", S::Integer),
                    ("max_chunk_bytes", S::Integer),
                    (
                        "disks",
                        S::map(
                            S::object([("driver", S::String), ("visibility", S::String)]).open(),
                        ),
                    ),
                ]),
                |value| {
                    let config: StorageConfig =
                        serde_json::from_value(value.clone()).map_err(|_| {
                            Error::Config(crate::config::ConfigError::Invalid(
                                "storage configuration is invalid".into(),
                            ))
                        })?;
                    validate_storage_config(&config)
                },
            )?;
            let custom = self.drivers.clone();
            app.singleton::<FilesystemManager, _>(move |resolver| {
                let custom = custom.clone();
                Box::pin(async move {
                    let mut drivers = StorageDrivers::default();
                    #[cfg(feature = "storage-local")]
                    drivers.register(FsDriver)?;
                    #[cfg(not(feature = "storage-local"))]
                    drivers.register(LocalUnavailableDriver)?;
                    #[cfg(feature = "storage-s3")]
                    drivers.register(S3Driver)?;
                    #[cfg(not(feature = "storage-s3"))]
                    drivers.register(S3UnavailableDriver)?;
                    for driver in custom {
                        drivers.register_shared(driver)?;
                    }
                    Ok(Arc::new(FilesystemManager::managed(resolver, drivers)))
                })
            })
            .await?;
            app.require::<FilesystemManager>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let storage = app.resolve::<FilesystemManager>().await?;
            let _ = storage.default_disk().await?;
            let final_storage = Arc::clone(&storage);
            app.validate_at_finalize(move || {
                Box::pin(async move { final_storage.validate_configured() })
            });
            Ok(())
        })
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::foundation::BootContext;
    use crate::foundation::ProviderBuilder;
    use crate::support::ServiceProvider;
    use figment::{Figment, providers::Serialized};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct DropCheck(Arc<StorageManager>, Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DropCheck {
        fn drop(&mut self) {
            self.1
                .store(self.0.drivers.try_lock().is_ok(), Ordering::SeqCst);
        }
    }
    impl StorageDriver for DropCheck {
        fn name(&self) -> &'static str {
            "drop-check"
        }
        fn build<'a>(
            &'a self,
            _: &'a StorageDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
            Box::pin(async { Err(Error::custom("unused", "unused")) })
        }
    }
    struct Replacement;
    impl StorageDriver for Replacement {
        fn name(&self) -> &'static str {
            "drop-check"
        }
        fn build<'a>(
            &'a self,
            _: &'a StorageDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
            Box::pin(async { Err(Error::custom("unused", "unused")) })
        }
    }
    #[tokio::test]
    async fn replacing_driver_drops_old_creator_outside_registry_lock() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(
            FilesystemServiceProvider::new().with_driver(Custom(Arc::new(AtomicUsize::new(0)))),
        );
        let runtime = builder
            .start(sources(json!({"first":{"driver":"custom"}})))
            .await?;
        let storage = runtime.container().resolve::<FilesystemManager>().await?;
        let manager = Arc::clone(&storage.manager);
        let unlocked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        storage.extend(DropCheck(manager, Arc::clone(&unlocked)))?;
        storage.extend(Replacement)?;
        assert!(unlocked.load(Ordering::SeqCst));
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct Backend(Arc<AtomicUsize>);
    impl Drop for Backend {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl StorageBackend for Backend {
        async fn put(
            &self,
            _: &StoragePath,
            _: Option<&str>,
            _: Option<super::Visibility>,
            _: StorageStream,
            _: UploadCancellation,
        ) -> Result<StorageMetadata> {
            Err(Error::custom("test", "unused"))
        }
        async fn open(&self, _: &StoragePath, _: Option<&str>) -> Result<StorageReader> {
            Err(Error::custom("test", "unused"))
        }
        async fn stat(&self, _: &StoragePath) -> Result<StorageMetadata> {
            Err(Error::custom("test", "unused"))
        }
        async fn delete(&self, _: &StoragePath) -> Result<()> {
            Ok(())
        }
    }
    struct Custom(Arc<AtomicUsize>);
    impl StorageDriver for Custom {
        fn name(&self) -> &'static str {
            "custom"
        }
        fn build<'a>(
            &'a self,
            _: &'a StorageDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
            let dropped = Arc::clone(&self.0);
            Box::pin(async move { Ok(Arc::new(Backend(dropped)) as Arc<dyn StorageBackend>) })
        }
    }
    struct Earlier(Arc<AtomicUsize>);
    impl ServiceProvider for Earlier {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            let dropped = Arc::clone(&self.0);
            Box::pin(async move {
                app.resolve::<FilesystemManager>()
                    .await?
                    .extend(Custom(dropped))
            })
        }
    }
    #[allow(clippy::needless_pass_by_value, reason = "测试调用直接传入 JSON 值")]
    fn sources(disks: serde_json::Value) -> Figment {
        Figment::from(Serialized::defaults(
            json!({"storage":{"default_disk":"first","disks":disks}}),
        ))
    }
    struct Counted(Arc<AtomicUsize>);
    impl StorageDriver for Counted {
        fn name(&self) -> &'static str {
            "custom"
        }
        fn build<'a>(
            &'a self,
            _: &'a StorageDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(Backend(Arc::new(AtomicUsize::new(0)))) as Arc<dyn StorageBackend>)
            })
        }
    }
    struct InvalidOverlay;
    impl ServiceProvider for InvalidOverlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay(
                    "storage",
                    json!({"disks": {"second": {"driver": "missing"}}}),
                )
            })
        }
    }
    #[tokio::test]
    async fn late_overlay_unknown_driver_fails_final_validation() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(
                FilesystemServiceProvider::new().with_driver(Custom(Arc::new(AtomicUsize::new(0)))),
            )
            .provider(InvalidOverlay);
        let source = sources(json!({"first":{"driver":"custom"}}));
        assert!(builder.start(source).await.is_err());
    }

    #[tokio::test]
    async fn late_extend_preserves_old_and_purge_rebuilds_with_new_driver() -> Result<()> {
        let old = Arc::new(AtomicUsize::new(0));
        let new = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(FilesystemServiceProvider::new().with_driver(Counted(Arc::clone(&old))));
        let runtime = builder
            .start(sources(
                json!({"first":{"driver":"custom"},"second":{"driver":"custom"}}),
            ))
            .await?;
        let storage = runtime.container().resolve::<FilesystemManager>().await?;
        let first = storage.disk("first").await?;
        storage.extend(Counted(Arc::clone(&new)))?;
        assert!(Arc::ptr_eq(
            &first.backend,
            &storage.disk("first").await?.backend
        ));
        let (left, right) = tokio::join!(storage.disk("second"), storage.disk("second"));
        assert!(Arc::ptr_eq(&left?.backend, &right?.backend));
        assert_eq!(new.load(Ordering::SeqCst), 1);
        storage.purge("first")?;
        assert!(!Arc::ptr_eq(
            &first.backend,
            &storage.disk("first").await?.backend
        ));
        assert_eq!(new.load(Ordering::SeqCst), 2);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn provider_uses_custom_and_earlier_drivers_for_named_disks() -> Result<()> {
        for earlier in [false, true] {
            let dropped = Arc::new(AtomicUsize::new(0));
            let mut builder = ProviderBuilder::new();
            if earlier {
                builder
                    .provider(Earlier(Arc::clone(&dropped)))
                    .provider(FilesystemServiceProvider::new());
            } else {
                builder.provider(
                    FilesystemServiceProvider::new().with_driver(Custom(Arc::clone(&dropped))),
                );
            }
            let runtime = builder
                .start(sources(
                    json!({"first":{"driver":"custom"},"second":{"driver":"custom"}}),
                ))
                .await?;
            let storage = runtime.container().resolve::<FilesystemManager>().await?;
            assert!(storage.default_disk().await.is_ok());
            assert!(storage.disk("second").await.is_ok());
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
                .await?;
        }
        Ok(())
    }
    #[tokio::test]
    async fn provider_rejects_conflicts_and_releases_constructed_backends_on_failure() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut duplicate = ProviderBuilder::new();
        duplicate.provider(
            FilesystemServiceProvider::new()
                .with_driver(Custom(Arc::clone(&dropped)))
                .with_driver(Custom(Arc::clone(&dropped))),
        );
        assert!(
            duplicate
                .start(sources(json!({"first":{"driver":"custom"}})))
                .await
                .is_err()
        );
        let mut missing = ProviderBuilder::new();
        missing.provider(FilesystemServiceProvider::new());
        assert!(
            missing
                .start(sources(json!({"first":{"driver":"unknown"}})))
                .await
                .is_err()
        );
        let mut invalid = ProviderBuilder::new();
        invalid.provider(FilesystemServiceProvider::new());
        assert!(
            invalid
                .start(sources(json!({"other":{"driver":"custom"}})))
                .await
                .is_err()
        );
    }
}

fn validate_disk_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
    {
        return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
            "invalid storage disk name `{name}`"
        ))));
    }
    Ok(())
}

fn valid_driver_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

fn validate_content_type(content_type: &str) -> Result<()> {
    let valid_token_byte =
        |byte: u8| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte);
    let valid = content_type.split_once('/').is_some_and(|(kind, subtype)| {
        !kind.is_empty()
            && !subtype.is_empty()
            && !subtype.contains('/')
            && kind.bytes().all(valid_token_byte)
            && subtype.bytes().all(valid_token_byte)
    });
    if content_type.is_empty()
        || content_type.len() > 255
        || content_type
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        || !valid
    {
        return Err(Error::custom(
            "storage_invalid_content_type",
            "upload content type is invalid",
        ));
    }
    Ok(())
}

fn unsupported_storage_capability(name: &str) -> Error {
    Error::custom(
        "storage_capability_unsupported",
        format!("storage backend does not support {name}"),
    )
}

#[cfg(feature = "storage-local")]
mod opendal_driver {
    use super::{
        BoxFuture, Error, Result, StorageBackend, StorageDriver, StorageDriverConfig,
        StorageMetadata, StoragePath, StorageReader, StorageStream, UploadCancellation, Visibility,
        async_trait, unsupported_storage_capability,
    };
    #[cfg(test)]
    use super::{UPLOAD_FAILED, UPLOAD_FINISHED, UPLOAD_PENDING, UploadCancellationGuard};
    use futures_util::StreamExt;
    use opendal::{Operator, services};
    #[cfg(feature = "storage-s3")]
    use serde::de::DeserializeOwned;
    use std::path::PathBuf;
    #[cfg(test)]
    use std::sync::atomic::AtomicU8;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

    pub struct FsDriver;

    #[async_trait::async_trait]
    impl StorageDriver for FsDriver {
        fn name(&self) -> &'static str {
            "fs"
        }

        fn validate(&self, config: &StorageDriverConfig) -> Result<()> {
            config.reject_unknown_options(&["root"])?;
            let _: Option<String> = config.option("root")?;
            Ok(())
        }
        fn build<'a>(
            &'a self,
            config: &'a StorageDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
            Box::pin(async move {
                config.reject_unknown_options(&["root"])?;
                let root = config
                    .option::<String>("root")?
                    .unwrap_or_else(|| "storage/app".into());
                let root = PathBuf::from(root);
                let root = if root.is_absolute() {
                    root
                } else {
                    std::env::current_dir().map_err(Error::internal)?.join(root)
                };
                if std::fs::symlink_metadata(&root)
                    .is_ok_and(|entry| entry.file_type().is_symlink())
                {
                    return Err(Error::custom(
                        "storage_invalid_root",
                        "storage root must not be a symlink",
                    ));
                }
                let root = root.to_string_lossy().into_owned();
                let staging_base = prepare_staging_base(std::path::Path::new(&root)).await?;
                cleanup_stale_staging(&staging_base)?;
                let staging = create_staging_directory(&staging_base)?;
                let staging = staging.to_string_lossy().into_owned();
                let operator = Operator::new(
                    services::Fs::default()
                        .root(&root)
                        .atomic_write_dir(&staging),
                )
                .map_err(map_opendal_error)?;
                Ok(Arc::new(OpenDalBackend {
                    operator,
                    staging_path: Some(PathBuf::from(staging)),
                    local_root: Some(PathBuf::from(root)),
                    #[cfg(feature = "storage-s3")]
                    s3_acl: None,
                }) as Arc<dyn StorageBackend>)
            })
        }
    }

    #[cfg(feature = "storage-s3")]
    pub struct S3Driver;

    #[cfg(feature = "storage-s3")]
    #[async_trait]
    impl StorageDriver for S3Driver {
        fn name(&self) -> &'static str {
            "s3"
        }

        fn validate(&self, config: &StorageDriverConfig) -> Result<()> {
            config.reject_unknown_options(&[
                "bucket",
                "region",
                "endpoint",
                "access_key_id",
                "secret_access_key",
                "root",
            ])?;
            for key in [
                "bucket",
                "region",
                "endpoint",
                "access_key_id",
                "secret_access_key",
            ] {
                let value: Option<String> = config.option(key)?;
                if value.as_deref().is_none_or(|value| value.trim().is_empty()) {
                    return Err(Error::Config(crate::config::ConfigError::Invalid(
                        "S3 required option is missing or empty".into(),
                    )));
                }
            }
            let endpoint: Option<String> = config.option("endpoint")?;
            let endpoint = endpoint.ok_or_else(|| {
                Error::Config(crate::config::ConfigError::Invalid(
                    "S3 endpoint is missing".into(),
                ))
            })?;
            let url = url::Url::parse(&endpoint).map_err(|_| {
                Error::Config(crate::config::ConfigError::Invalid(
                    "S3 endpoint is invalid".into(),
                ))
            })?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(Error::Config(crate::config::ConfigError::Invalid(
                    "S3 endpoint is invalid".into(),
                )));
            }
            let _: Option<String> = config.option("root")?;
            Ok(())
        }
        fn build<'a>(
            &'a self,
            config: &'a StorageDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
            Box::pin(async move {
                config.reject_unknown_options(&[
                    "bucket",
                    "region",
                    "endpoint",
                    "access_key_id",
                    "secret_access_key",
                    "root",
                ])?;
                opendal::install_default();
                let bucket = required_option::<String>(config, "bucket")?;
                let region = required_option::<String>(config, "region")?;
                let endpoint = required_option::<String>(config, "endpoint")?;
                let access_key_id = required_option::<String>(config, "access_key_id")?;
                let secret_access_key = required_option::<String>(config, "secret_access_key")?;
                let root = config.option::<String>("root")?.unwrap_or_default();
                if [
                    bucket.as_str(),
                    region.as_str(),
                    endpoint.as_str(),
                    access_key_id.as_str(),
                    secret_access_key.as_str(),
                ]
                .iter()
                .any(|value| value.trim().is_empty())
                {
                    return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
                        "storage disk `{}` requires non-empty S3 options",
                        config.disk_name()
                    ))));
                }
                let endpoint_url = url::Url::parse(&endpoint).map_err(|_| {
                    Error::Config(crate::config::ConfigError::Invalid(format!(
                        "storage disk `{}` has an invalid S3 endpoint",
                        config.disk_name()
                    )))
                })?;
                if !matches!(endpoint_url.scheme(), "http" | "https")
                    || endpoint_url.host_str().is_none()
                {
                    return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
                        "storage disk `{}` requires an HTTP(S) S3 endpoint",
                        config.disk_name()
                    ))));
                }
                let operator = Operator::new(
                    services::S3::default()
                        .bucket(&bucket)
                        .region(&region)
                        .endpoint(&endpoint)
                        .access_key_id(&access_key_id)
                        .secret_access_key(&secret_access_key)
                        .root(&root)
                        .disable_config_load(),
                )
                .map_err(map_opendal_error)?;
                if !operator.info().capability().copy {
                    return Err(unsupported_storage_capability(
                        "atomic S3 publication by copy",
                    ));
                }
                if !operator.info().capability().presign_read {
                    return Err(unsupported_storage_capability("presigned read"));
                }
                let acl_config = aws_sdk_s3::Config::builder()
                    .behavior_version_latest()
                    .timeout_config(
                        aws_sdk_s3::config::timeout::TimeoutConfig::builder()
                            .operation_timeout(Duration::from_secs(30))
                            .operation_attempt_timeout(Duration::from_secs(30))
                            .build(),
                    )
                    .retry_config(
                        aws_sdk_s3::config::retry::RetryConfig::standard().with_max_attempts(1),
                    )
                    .region(aws_sdk_s3::config::Region::new(region))
                    .endpoint_url(endpoint)
                    .force_path_style(true)
                    .credentials_provider(aws_sdk_s3::config::Credentials::new(
                        access_key_id,
                        secret_access_key,
                        None,
                        None,
                        "openrust-storage",
                    ))
                    .build();
                Ok(Arc::new(OpenDalBackend {
                    operator,
                    staging_path: None,
                    local_root: None,
                    s3_acl: Some(S3Acl {
                        client: aws_sdk_s3::Client::from_conf(acl_config),
                        bucket,
                        root,
                    }),
                }) as Arc<dyn StorageBackend>)
            })
        }
    }

    #[cfg(feature = "storage-s3")]
    fn required_option<T: DeserializeOwned>(config: &StorageDriverConfig, key: &str) -> Result<T> {
        config.option(key)?.ok_or_else(|| {
            Error::Config(crate::config::ConfigError::Invalid(format!(
                "storage disk `{}` requires option `{key}`",
                config.disk_name()
            )))
        })
    }

    pub struct OpenDalBackend {
        operator: Operator,
        staging_path: Option<PathBuf>,
        local_root: Option<PathBuf>,
        #[cfg(feature = "storage-s3")]
        s3_acl: Option<S3Acl>,
    }

    #[cfg(feature = "storage-s3")]
    #[derive(Clone)]
    struct S3Acl {
        client: aws_sdk_s3::Client,
        bucket: String,
        root: String,
    }

    impl Drop for OpenDalBackend {
        fn drop(&mut self) {
            if let Some(staging_path) = &self.staging_path {
                let _ = std::fs::remove_dir_all(staging_path);
            }
        }
    }

    fn cleanup_stale_staging(base: &std::path::Path) -> Result<()> {
        let entries = std::fs::read_dir(base).map_err(Error::internal)?;
        let cutoff = std::time::SystemTime::now()
            .checked_sub(Duration::from_hours(24))
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        for entry in entries {
            cleanup_staging_entry(base, &entry.map_err(Error::internal)?, cutoff)?;
        }
        Ok(())
    }

    async fn prepare_staging_base(root: &std::path::Path) -> Result<PathBuf> {
        use std::io::Write;

        const OWNER: &[u8] = b"storage-staging-v1\n";
        let base = root.join(".staging");
        std::fs::create_dir_all(root).map_err(Error::internal)?;
        match std::fs::create_dir(&base) {
            Ok(()) => {
                // 仅新建者设置目录权限；现存同名业务目录绝不接管或改权限。
                let directory = std::fs::File::open(&base).map_err(Error::internal)?;
                let entry = std::fs::symlink_metadata(&base).map_err(Error::internal)?;
                if !entry.is_dir() || entry.file_type().is_symlink() {
                    return Err(unowned_staging_root());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::{MetadataExt, PermissionsExt};
                    let opened = directory.metadata().map_err(Error::internal)?;
                    if entry.dev() != opened.dev() || entry.ino() != opened.ino() {
                        return Err(unowned_staging_root());
                    }
                    directory
                        .set_permissions(std::fs::Permissions::from_mode(0o700))
                        .map_err(Error::internal)?;
                }
                let marker = base.join(".storage-owner");
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(marker)
                    .map_err(Error::internal)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(std::fs::Permissions::from_mode(0o600))
                        .map_err(Error::internal)?;
                }
                file.write_all(OWNER).map_err(Error::internal)?;
                file.sync_all().map_err(Error::internal)?;
                Ok(base)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // 并发首次创建时 marker 可能尚未落盘；只读等待有界，失败即拒绝。
                for _ in 0..100 {
                    let entry = std::fs::symlink_metadata(&base).map_err(Error::internal)?;
                    if !entry.is_dir() || entry.file_type().is_symlink() {
                        return Err(unowned_staging_root());
                    }
                    let marker = base.join(".storage-owner");
                    if let Ok(metadata) = std::fs::symlink_metadata(&marker)
                        && metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.len() == OWNER.len() as u64
                        && std::fs::read(&marker).is_ok_and(|bytes| bytes == OWNER)
                    {
                        return Ok(base);
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(unowned_staging_root())
            }
            Err(error) => Err(Error::internal(error)),
        }
    }

    fn unowned_staging_root() -> Error {
        Error::custom(
            "storage_staging_unowned",
            "storage staging root already exists without a valid owner marker",
        )
    }

    fn cleanup_staging_entry(
        base: &std::path::Path,
        entry: &std::fs::DirEntry,
        cutoff: std::time::SystemTime,
    ) -> Result<()> {
        let path = entry.path();
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("run-") {
            return Ok(());
        }
        let Some(file_type) = staging_entry_or_missing(base, entry.file_type())? else {
            return Ok(());
        };
        if !file_type.is_dir() {
            return Ok(());
        }
        let Some(metadata) = staging_entry_or_missing(base, entry.metadata())? else {
            return Ok(());
        };
        let modified = metadata.modified().map_err(Error::internal)?;
        if modified < cutoff {
            let _ = staging_entry_or_missing(base, std::fs::remove_dir_all(path))?;
        }
        Ok(())
    }

    // 另一后端的 Drop 可在列举后移除自身目录；仅条目消失表示已清理。
    // 权限、IO 故障及 staging 根目录的读取错误仍阻止后端初始化。
    fn staging_entry_or_missing<T>(
        base: &std::path::Path,
        result: std::io::Result<T>,
    ) -> Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // 非原子复核仅确认扫描根目录此刻仍可访问；不承诺抵御目录替换。
                let _ = std::fs::read_dir(base).map_err(Error::internal)?;
                Ok(None)
            }
            Err(error) => Err(Error::internal(error)),
        }
    }

    #[cfg(test)]
    #[test]
    fn staging_entry_disappearing_after_enumeration_is_already_clean() -> Result<()> {
        let base = tempfile::tempdir().map_err(Error::internal)?;
        let run = base.path().join("run-owned");
        std::fs::create_dir(&run).map_err(Error::internal)?;
        let entry = std::fs::read_dir(base.path())
            .map_err(Error::internal)?
            .next()
            .ok_or_else(|| Error::internal(std::io::Error::other("missing fixture entry")))?
            .map_err(Error::internal)?;
        // 模拟另一后端在 read_dir 已列举、读取 metadata 前完成 Drop。
        std::fs::remove_dir(&run).map_err(Error::internal)?;
        cleanup_staging_entry(base.path(), &entry, std::time::SystemTime::now())
    }

    fn create_staging_directory(base: &std::path::Path) -> Result<PathBuf> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(Error::internal)?
            .as_nanos();
        for _ in 0..16 {
            let sequence = next_staging_id()?;
            let path = base.join(format!("run-{}-{timestamp}-{sequence}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                            .map_err(Error::internal)?;
                    }
                    return Ok(path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(Error::internal(error)),
            }
        }
        Err(Error::internal(std::io::Error::other(
            "could not reserve a unique staging directory",
        )))
    }

    fn next_staging_id() -> Result<u64> {
        loop {
            let current = NEXT_STAGING_ID.load(Ordering::Relaxed);
            let next = current
                .checked_add(1)
                .ok_or_else(|| Error::internal(std::io::Error::other("staging ID exhausted")))?;
            if NEXT_STAGING_ID
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(current);
            }
        }
    }

    #[cfg(test)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_filesystem_driver_builds_reserve_distinct_staging_directories() -> Result<()>
    {
        use std::collections::BTreeMap;

        let root = std::env::temp_dir().join(format!(
            "openrust-fs-build-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(Error::internal)?
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).map_err(Error::internal)?;
        let config = Arc::new(StorageDriverConfig {
            disk_name: "local".to_owned(),
            options: BTreeMap::from([(
                "root".to_owned(),
                serde_json::Value::String(root.to_string_lossy().into_owned()),
            )]),
        });
        let driver = Arc::new(FsDriver);
        let barrier = Arc::new(tokio::sync::Barrier::new(64));
        let mut builds = Vec::with_capacity(64);
        for _ in 0..64 {
            let config = Arc::clone(&config);
            let driver = Arc::clone(&driver);
            let barrier = Arc::clone(&barrier);
            builds.push(tokio::spawn(async move {
                barrier.wait().await;
                driver.build(&config).await
            }));
        }
        let mut backends = Vec::with_capacity(builds.len());
        for build in builds {
            backends.push(build.await.map_err(Error::internal)??);
        }
        drop(backends);
        std::fs::remove_dir_all(root).map_err(Error::internal)?;
        Ok(())
    }

    #[cfg(test)]
    #[tokio::test]
    async fn existing_business_staging_is_not_claimed_or_cleaned() -> Result<()> {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let business = root.path().join(".staging");
        std::fs::create_dir(&business).map_err(Error::internal)?;
        let object = business.join("run-backup");
        std::fs::write(&object, b"business data").map_err(Error::internal)?;
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(&business)
                .map_err(Error::internal)?
                .permissions()
                .mode()
        };
        let config = StorageDriverConfig {
            disk_name: "local".to_owned(),
            options: std::collections::BTreeMap::from([(
                "root".to_owned(),
                serde_json::Value::String(root.path().to_string_lossy().into_owned()),
            )]),
        };
        assert!(FsDriver.build(&config).await.is_err());
        assert_eq!(
            std::fs::read(&object).map_err(Error::internal)?,
            b"business data"
        );
        assert!(!business.join(".storage-owner").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&business)
                    .map_err(Error::internal)?
                    .permissions()
                    .mode(),
                permissions
            );
        }
        Ok(())
    }

    #[cfg(all(test, unix))]
    #[tokio::test]
    async fn symlinked_staging_is_not_claimed() -> Result<()> {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let outside = tempfile::tempdir().map_err(Error::internal)?;
        let object = outside.path().join("run-backup");
        std::fs::write(&object, b"outside data").map_err(Error::internal)?;
        std::os::unix::fs::symlink(outside.path(), root.path().join(".staging"))
            .map_err(Error::internal)?;
        let config = StorageDriverConfig {
            disk_name: "local".to_owned(),
            options: std::collections::BTreeMap::from([(
                "root".to_owned(),
                serde_json::Value::String(root.path().to_string_lossy().into_owned()),
            )]),
        };
        assert!(FsDriver.build(&config).await.is_err());
        assert_eq!(
            std::fs::read(&object).map_err(Error::internal)?,
            b"outside data"
        );
        assert!(!outside.path().join(".storage-owner").exists());
        Ok(())
    }

    #[cfg(test)]
    #[tokio::test]
    async fn owned_staging_survives_backend_restart() -> Result<()> {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let config = StorageDriverConfig {
            disk_name: "local".to_owned(),
            options: std::collections::BTreeMap::from([(
                "root".to_owned(),
                serde_json::Value::String(root.path().to_string_lossy().into_owned()),
            )]),
        };
        drop(FsDriver.build(&config).await?);
        assert_eq!(
            std::fs::read(root.path().join(".staging/.storage-owner")).map_err(Error::internal)?,
            b"storage-staging-v1\n"
        );
        drop(FsDriver.build(&config).await?);
        Ok(())
    }

    #[cfg(test)]
    #[test]
    fn staging_base_disappearing_after_enumeration_remains_a_failure() -> Result<()> {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let base = root.path().join("staging");
        std::fs::create_dir_all(base.join("run-owned")).map_err(Error::internal)?;
        let entry = std::fs::read_dir(&base)
            .map_err(Error::internal)?
            .next()
            .ok_or_else(|| Error::internal(std::io::Error::other("missing fixture entry")))?
            .map_err(Error::internal)?;
        std::fs::remove_dir_all(&base).map_err(Error::internal)?;
        assert!(cleanup_staging_entry(&base, &entry, std::time::SystemTime::now()).is_err());
        std::fs::write(&base, b"not a directory").map_err(Error::internal)?;
        assert!(
            staging_entry_or_missing::<()>(
                &base,
                Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            )
            .is_err()
        );
        Ok(())
    }

    #[cfg(test)]
    #[test]
    fn staging_cleanup_preserves_non_missing_errors_and_missing_base_failure() -> Result<()> {
        let base = tempfile::tempdir().map_err(Error::internal)?;
        let error = staging_entry_or_missing::<()>(
            base.path(),
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        )
        .err()
        .ok_or_else(|| Error::internal(std::io::Error::other("permission failure was ignored")))?;
        assert!(
            std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<std::io::Error>())
                .is_some_and(|source| source.kind() == std::io::ErrorKind::PermissionDenied)
        );
        assert!(
            staging_entry_or_missing::<()>(
                base.path(),
                Err(std::io::Error::from(std::io::ErrorKind::NotFound,))
            )?
            .is_none()
        );
        assert!(cleanup_stale_staging(&base.path().join("missing-base")).is_err());
        Ok(())
    }

    #[cfg(test)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_filesystem_driver_builds_and_drops_share_staging_root() -> Result<()> {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let config = Arc::new(StorageDriverConfig {
            disk_name: "local".to_owned(),
            options: std::collections::BTreeMap::from([(
                "root".to_owned(),
                serde_json::Value::String(root.path().to_string_lossy().into_owned()),
            )]),
        });
        let barrier = Arc::new(tokio::sync::Barrier::new(16));
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let config = Arc::clone(&config);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                for _ in 0..16 {
                    let backend = FsDriver.build(&config).await?;
                    tokio::task::yield_now().await;
                    drop(backend);
                }
                Ok::<_, Error>(())
            }));
        }
        for task in tasks {
            task.await.map_err(Error::internal)??;
        }
        for entry in std::fs::read_dir(root.path().join(".staging")).map_err(Error::internal)? {
            assert_eq!(
                entry.map_err(Error::internal)?.file_name(),
                std::ffi::OsStr::new(".storage-owner")
            );
        }
        Ok(())
    }

    async fn write_object(
        operator: Operator,
        path: String,
        content_type: Option<String>,
        mut body: StorageStream,
        cancellation: &mut UploadCancellation,
    ) -> Result<StorageMetadata> {
        let options = opendal::options::WriteOptions {
            content_type,
            ..Default::default()
        };
        let local = operator.info().scheme() == services::FS_SCHEME;
        let mut writer = operator
            .writer_options(&path, options)
            .await
            .map_err(map_opendal_error)?;
        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => {
                    let _ = writer.abort().await;
                    return Err(Error::custom("storage_upload_cancelled", "upload was cancelled before publication"));
                }
                next = body.next() => next,
            };
            match next {
                Some(Ok(chunk)) => {
                    let result = tokio::select! {
                        biased;
                        // Fs 首写异步创建随机临时文件，须等 writer 接管路径后再 abort。
                        // 本地每块结束后，在读取下一块前优先处理取消。
                        () = cancellation.cancelled(), if !local => {
                            let _ = writer.abort().await;
                            return Err(Error::custom("storage_upload_cancelled", "upload was cancelled before publication"));
                        }
                        result = writer.write(chunk) => result,
                    };
                    if let Err(error) = result {
                        let _ = writer.abort().await;
                        return Err(map_opendal_error(error));
                    }
                }
                Some(Err(error)) => {
                    let _ = writer.abort().await;
                    return Err(error);
                }
                None => break,
            }
        }
        match writer.close().await {
            Ok(metadata) => Ok(metadata_from_opendal(metadata)),
            Err(error) => {
                let _ = writer.abort().await;
                Err(map_opendal_error(error))
            }
        }
    }

    #[cfg(all(test, unix))]
    async fn advance_one_local_io(
        worker: std::pin::Pin<&mut impl std::future::Future>,
    ) -> Result<()> {
        // 唯一 blocking 线程在本次 poll 期间暂停，文件打开不可能在同一 poll 内返回。
        let (entered, held) = tokio::sync::oneshot::channel();
        let (release, resume) = tokio::sync::oneshot::channel::<()>();
        let holder = tokio::task::spawn_blocking(move || {
            let _ = entered.send(());
            let _ = resume.blocking_recv();
        });
        held.await.map_err(Error::internal)?;
        assert!(futures_util::poll!(worker).is_pending());
        drop(release);
        holder.await.map_err(Error::internal)?;
        // 已排队的 fs 操作先完成；尚未再次 poll worker，open 结果仍未交给 writer。
        tokio::task::spawn_blocking(|| {})
            .await
            .map_err(Error::internal)
    }

    #[cfg(all(test, unix))]
    #[test]
    fn cancelling_first_local_write_removes_the_file_created_by_that_write() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .map_err(Error::internal)?;
        runtime.block_on(async {
            let root = tempfile::tempdir().map_err(Error::internal)?;
            let staging = root.path().join(".staging/run");
            std::fs::create_dir_all(&staging).map_err(Error::internal)?;
            let operator = Operator::new(
                services::Fs::default()
                    .root(&root.path().to_string_lossy())
                    .atomic_write_dir(&staging.to_string_lossy()),
            )
            .map_err(map_opendal_error)?;
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let publication = Arc::new(AtomicU8::new(UPLOAD_PENDING));
            let guard = UploadCancellationGuard {
                sender: Some(sender),
                publication: Arc::clone(&publication),
            };
            let (next_chunk, mut polled_again) = tokio::sync::oneshot::channel();
            let body = futures_util::stream::iter([Ok(bytes::Bytes::from_static(b"data"))]).chain(
                futures_util::stream::once(async move {
                    let _ = next_chunk.send(());
                    std::future::pending::<Result<bytes::Bytes>>().await
                }),
            );
            let worker = write_local_object(
                operator,
                root.path().to_owned(),
                staging.clone(),
                "target".to_owned(),
                None,
                None,
                Box::pin(body),
                UploadCancellation {
                    receiver,
                    publication: Arc::clone(&publication),
                },
            );
            tokio::pin!(worker);
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            let staged = tokio::time::timeout_at(deadline, async {
                loop {
                    advance_one_local_io(worker.as_mut()).await?;
                    let mut entries = std::fs::read_dir(&staging).map_err(Error::internal)?;
                    if let Some(entry) = entries.next().transpose().map_err(Error::internal)? {
                        assert!(entry.file_type().map_err(Error::internal)?.is_file());
                        assert!(
                            entries
                                .next()
                                .transpose()
                                .map_err(Error::internal)?
                                .is_none()
                        );
                        return Ok::<_, Error>(entry.path());
                    }
                }
            })
            .await
            .map_err(Error::internal)??;
            assert!(matches!(
                polled_again.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            drop(guard);
            assert_eq!(publication.load(Ordering::Acquire), super::UPLOAD_CANCELLED);
            let error = tokio::time::timeout_at(deadline, worker)
                .await
                .map_err(Error::internal)?
                .err()
                .ok_or_else(|| {
                    Error::custom("storage_test_cancel_expected", "上传应在发布前取消")
                })?;
            assert_eq!(error.code(), "storage_upload_cancelled");
            assert!(!staged.try_exists().map_err(Error::internal)?);
            assert!(
                std::fs::read_dir(&staging)
                    .map_err(Error::internal)?
                    .next()
                    .transpose()
                    .map_err(Error::internal)?
                    .is_none()
            );
            assert!(
                !root
                    .path()
                    .join("target")
                    .try_exists()
                    .map_err(Error::internal)?
            );
            Ok(())
        })
    }

    #[cfg(feature = "storage-s3")]
    async fn cleanup_s3_staging(operator: &Operator, path: &str) {
        // 清理不影响已发布对象的成功结果；故障时由存储运维回收保留前缀。
        for _ in 0..3 {
            if matches!(
                tokio::time::timeout(Duration::from_secs(2), operator.delete(path)).await,
                Ok(Ok(()))
            ) {
                return;
            }
        }
        tracing::warn!(
            code = "storage_staging_cleanup_failed",
            "S3 暂存对象清理失败，需回收保留前缀中的滞留对象"
        );
    }

    #[cfg(feature = "storage-s3")]
    #[allow(clippy::too_many_lines)]
    async fn write_s3_object(
        operator: Operator,
        acl: S3Acl,
        path: String,
        content_type: Option<String>,
        visibility: Option<Visibility>,
        body: StorageStream,
        mut cancellation: UploadCancellation,
    ) -> Result<StorageMetadata> {
        use aws_sdk_s3::error::ProvideErrorMetadata;
        // 保留目录不能通过 StoragePath 构造；随机 key 不包含业务或租户标识。
        let staging = format!(".staging/{}", uuid::Uuid::now_v7());
        let staged = write_object(
            operator.clone(),
            staging.clone(),
            content_type,
            body,
            &mut cancellation,
        )
        .await;
        if let Err(error) = staged {
            cleanup_s3_staging(&operator, &staging).await;
            return Err(error);
        }
        let source = match operator.stat(&staging).await {
            Ok(metadata) => metadata_from_opendal(metadata),
            Err(error) => {
                cleanup_s3_staging(&operator, &staging).await;
                return Err(map_opendal_error(error));
            }
        };
        // EOF 与 finish 已完成，关闭 staging 仍不是 final key 的发布边界。
        // CAS 胜出后将 Copy 与清理交给独立任务，调用方丢弃 future 仍不打断提交。
        let publication = match cancellation.begin_publication() {
            Ok(publication) => publication,
            Err(error) => {
                cleanup_s3_staging(&operator, &staging).await;
                return Err(error);
            }
        };
        let cleanup_operator = operator.clone();
        let cleanup_key = staging.clone();
        let source_key = acl.key(&staging);
        let mut copy_source = format!("{}/{}", acl.bucket, encode_copy_source(&source_key));
        if let Some(version) = &source.version {
            copy_source.push_str("?versionId=");
            copy_source.push_str(&encode_copy_source(version));
        }
        let mut copy = acl
            .client
            .copy_object()
            .bucket(&acl.bucket)
            .key(acl.key(&path))
            .copy_source(copy_source)
            .metadata_directive(aws_sdk_s3::types::MetadataDirective::Copy);
        if let Some(etag) = &source.etag {
            copy = copy.copy_source_if_match(etag);
        }
        if let Some(visibility) = visibility {
            copy = copy.acl(visibility.s3_acl());
        }
        let publisher = tokio::spawn(async move {
            let result = copy.send().await
                .map(|result| StorageMetadata {
                    size: source.size,
                    content_type: source.content_type,
                    etag: result.copy_object_result().and_then(|copy| copy.e_tag()).map(str::to_owned),
                    version: result.version_id().map(str::to_owned),
                })
                .map_err(|error| match error.as_service_error().and_then(ProvideErrorMetadata::code) {
                    Some("AccessControlListNotSupported" | "InvalidBucketAclWithObjectOwnership" | "AccessDenied" | "AllAccessDisabled") =>
                        s3_acl_error(error.as_service_error().and_then(ProvideErrorMetadata::code), "copy"),
                    _ => Error::custom(
                        "storage_publication_uncertain",
                        "S3 发布请求失败，远端可能已提交完整对象；请读取目标确认结果，禁止补偿删除目标",
                    ),
                });
            match &result {
                Ok(_) => publication.complete(),
                Err(error) if error.code() == "storage_publication_uncertain" => {
                    publication.uncertain();
                }
                Err(_) => publication.fail(),
            }
            cleanup_s3_staging(&cleanup_operator, &cleanup_key).await;
            result
        });
        publisher.await.map_err(|_| {
            Error::custom(
                "storage_publication_uncertain",
                "S3 发布任务中断，远端提交状态不确定",
            )
        })?
    }

    #[cfg(feature = "storage-s3")]
    impl S3Acl {
        fn key(&self, path: &str) -> String {
            let prefix = self
                .root
                .split('/')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("/");
            if prefix.is_empty() {
                path.to_owned()
            } else {
                format!("{prefix}/{path}")
            }
        }

        async fn visibility(&self, path: &StoragePath) -> Result<Visibility> {
            use aws_sdk_s3::error::ProvideErrorMetadata;
            let result = self
                .client
                .get_object_acl()
                .bucket(&self.bucket)
                .key(self.key(path.as_str()))
                .send()
                .await
                .map_err(|error| {
                    s3_acl_error(
                        error
                            .as_service_error()
                            .and_then(ProvideErrorMetadata::code),
                        "read",
                    )
                })?;
            classify_s3_acl(result.owner(), result.grants.as_deref())
        }

        async fn set_visibility(&self, path: &StoragePath, visibility: Visibility) -> Result<()> {
            use aws_sdk_s3::error::ProvideErrorMetadata;
            self.client
                .put_object_acl()
                .bucket(&self.bucket)
                .key(self.key(path.as_str()))
                .acl(visibility.s3_acl())
                .send()
                .await
                .map_err(|error| {
                    s3_acl_error(
                        error
                            .as_service_error()
                            .and_then(ProvideErrorMetadata::code),
                        "change",
                    )
                })?;
            Ok(())
        }
    }

    #[cfg(feature = "storage-s3")]
    fn classify_s3_acl(
        owner: Option<&aws_sdk_s3::types::Owner>,
        grants: Option<&[aws_sdk_s3::types::Grant]>,
    ) -> Result<Visibility> {
        let invalid = || {
            Error::custom(
                "storage_acl_invalid_response",
                "S3 returned an incomplete or unrecognized object ACL",
            )
        };
        if owner.and_then(|owner| owner.id()).is_none_or(str::is_empty) {
            return Err(invalid());
        }
        let grants = grants
            .filter(|grants| !grants.is_empty())
            .ok_or_else(invalid)?;
        let mut public = false;
        for grant in grants {
            let grantee = grant.grantee().ok_or_else(invalid)?;
            let permission = grant.permission().ok_or_else(invalid)?.as_str();
            if !matches!(
                permission,
                "READ" | "FULL_CONTROL" | "READ_ACP" | "WRITE" | "WRITE_ACP"
            ) {
                return Err(invalid());
            }
            match grantee.r#type().as_str() {
                "Group" => {
                    let uri = grantee
                        .uri()
                        .filter(|uri| !uri.is_empty())
                        .ok_or_else(invalid)?;
                    match uri {
                        "http://acs.amazonaws.com/groups/global/AllUsers" => {
                            public |= matches!(permission, "READ" | "FULL_CONTROL");
                        }
                        "http://acs.amazonaws.com/groups/global/AuthenticatedUsers"
                        | "http://acs.amazonaws.com/groups/s3/LogDelivery" => {}
                        _ => return Err(invalid()),
                    }
                }
                "CanonicalUser" if grantee.id().is_some_and(|id| !id.is_empty()) => {}
                "AmazonCustomerByEmail"
                    if grantee
                        .email_address()
                        .is_some_and(|email| !email.is_empty()) => {}
                _ => return Err(invalid()),
            }
        }
        Ok(if public {
            Visibility::Public
        } else {
            Visibility::Private
        })
    }

    #[cfg(feature = "storage-s3")]
    fn s3_acl_error(code: Option<&str>, operation: &str) -> Error {
        let (stable, message) = match code {
            Some("AccessControlListNotSupported" | "InvalidBucketAclWithObjectOwnership") => (
                "storage_acl_disabled",
                "S3 bucket does not allow object ACLs",
            ),
            Some("AccessDenied" | "AllAccessDisabled") => (
                "storage_acl_access_denied",
                "S3 object ACL access was denied",
            ),
            Some("NoSuchKey" | "NotFound") => {
                ("storage_object_not_found", "S3 object was not found")
            }
            _ => ("storage_acl_failed", "S3 object ACL operation failed"),
        };
        Error::custom(stable, format!("{message} during {operation}"))
    }

    #[cfg(feature = "storage-s3")]
    impl Visibility {
        fn s3_acl(self) -> aws_sdk_s3::types::ObjectCannedAcl {
            match self {
                Self::Public => aws_sdk_s3::types::ObjectCannedAcl::PublicRead,
                Self::Private => aws_sdk_s3::types::ObjectCannedAcl::Private,
            }
        }
    }

    #[cfg(feature = "storage-s3")]
    fn encode_copy_source(value: &str) -> String {
        let mut encoded = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                encoded.push(char::from(byte));
            } else {
                use std::fmt::Write;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
        encoded
    }

    #[cfg(all(test, feature = "storage-s3"))]
    #[test]
    fn copy_source_encodes_reserved_bytes_without_erasing_key_slashes() {
        assert_eq!(
            encode_copy_source("prefix/a b/%#?.txt"),
            "prefix/a%20b/%25%23%3F.txt"
        );
        assert_eq!(
            s3_acl_error(Some("AccessControlListNotSupported"), "copy").code(),
            "storage_acl_disabled"
        );
        assert_eq!(
            s3_acl_error(Some("AccessDenied"), "change").code(),
            "storage_acl_access_denied"
        );
    }

    #[cfg(all(test, feature = "storage-s3"))]
    #[test]
    fn acl_classifier_rejects_missing_fields_and_recognizes_public_grant() -> Result<()> {
        use aws_sdk_s3::types::{Grant, Grantee, Owner, Permission, Type};
        let owner = Owner::builder().id("owner").build();
        let owner_grant = Grant::builder()
            .grantee(
                Grantee::builder()
                    .r#type(Type::CanonicalUser)
                    .id("owner")
                    .build()
                    .map_err(Error::internal)?,
            )
            .permission(Permission::FullControl)
            .build();
        assert_eq!(
            classify_s3_acl(Some(&owner), Some(std::slice::from_ref(&owner_grant))).ok(),
            Some(Visibility::Private)
        );
        assert_eq!(
            classify_s3_acl(None, Some(std::slice::from_ref(&owner_grant)))
                .err()
                .map(|e| e.code().to_owned()),
            Some("storage_acl_invalid_response".to_owned())
        );
        assert!(classify_s3_acl(Some(&owner), None).is_err());
        assert!(classify_s3_acl(Some(&owner), Some(&[Grant::builder().build()])).is_err());
        let unknown = Grant::builder()
            .grantee(
                Grantee::builder()
                    .r#type(Type::from("FutureGrantee"))
                    .id("someone")
                    .build()
                    .map_err(Error::internal)?,
            )
            .permission(Permission::Read)
            .build();
        assert!(classify_s3_acl(Some(&owner), Some(&[unknown])).is_err());
        for uri in [
            "",
            "garbage",
            "http://acs.amazonaws.com/groups/global/Unknown",
        ] {
            let grant = Grant::builder()
                .grantee(
                    Grantee::builder()
                        .r#type(Type::Group)
                        .uri(uri)
                        .build()
                        .map_err(Error::internal)?,
                )
                .permission(Permission::Read)
                .build();
            assert_eq!(
                classify_s3_acl(Some(&owner), Some(&[grant]))
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("storage_acl_invalid_response".to_owned())
            );
        }
        for uri in [
            "http://acs.amazonaws.com/groups/global/AuthenticatedUsers",
            "http://acs.amazonaws.com/groups/s3/LogDelivery",
        ] {
            let grant = Grant::builder()
                .grantee(
                    Grantee::builder()
                        .r#type(Type::Group)
                        .uri(uri)
                        .build()
                        .map_err(Error::internal)?,
                )
                .permission(Permission::Read)
                .build();
            assert_eq!(
                classify_s3_acl(Some(&owner), Some(&[grant])).ok(),
                Some(Visibility::Private)
            );
        }
        let public_grant = Grant::builder()
            .grantee(
                Grantee::builder()
                    .r#type(Type::Group)
                    .uri("http://acs.amazonaws.com/groups/global/AllUsers")
                    .build()
                    .map_err(Error::internal)?,
            )
            .permission(Permission::Read)
            .build();
        assert_eq!(
            classify_s3_acl(Some(&owner), Some(&[owner_grant, public_grant])).ok(),
            Some(Visibility::Public)
        );
        Ok(())
    }

    #[cfg(unix)]
    fn local_parent(
        root: &std::path::Path,
        path: &str,
        visibility: Visibility,
        create: bool,
    ) -> Result<(std::os::fd::OwnedFd, String)> {
        use rustix::fs::{Mode, OFlags, fchmod, fstat, mkdirat, openat};
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut directory =
            openat(rustix::fs::CWD, root, flags, Mode::empty()).map_err(Error::internal)?;
        let root_stat = fstat(&directory).map_err(Error::internal)?;
        if visibility == Visibility::Public
            && !Mode::from_raw_mode(root_stat.st_mode).contains(Mode::XOTH)
        {
            return Err(unsupported_storage_capability(
                "public access through private storage root",
            ));
        }
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return Ok((directory, part.to_owned()));
            }
            let next = match openat(&directory, part, flags, Mode::empty()) {
                Ok(next) => next,
                Err(rustix::io::Errno::NOENT) if create => {
                    let mode = local_directory_mode(visibility);
                    let created = match mkdirat(&directory, part, mode) {
                        Ok(()) => true,
                        Err(rustix::io::Errno::EXIST) => false,
                        Err(error) => return Err(Error::internal(error)),
                    };
                    let next =
                        openat(&directory, part, flags, Mode::empty()).map_err(Error::internal)?;
                    // 新建目录受 umask 影响；已有目录绝不被放宽。
                    if created {
                        fchmod(&next, mode).map_err(Error::internal)?;
                    }
                    next
                }
                Err(error) => return Err(Error::internal(error)),
            };
            if visibility == Visibility::Public {
                let stat = fstat(&next).map_err(Error::internal)?;
                if !Mode::from_raw_mode(stat.st_mode).contains(Mode::XOTH) {
                    return Err(unsupported_storage_capability(
                        "public access through private parent directory",
                    ));
                }
            }
            directory = next;
        }
        Err(Error::custom(
            "storage_invalid_path",
            "storage path is empty",
        ))
    }

    #[cfg(unix)]
    fn local_directory_mode(visibility: Visibility) -> rustix::fs::Mode {
        use rustix::fs::Mode;
        let owner = Mode::RUSR | Mode::WUSR | Mode::XUSR;
        if visibility == Visibility::Public {
            owner | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH
        } else {
            owner
        }
    }

    #[cfg(unix)]
    fn local_file_mode(visibility: Visibility) -> rustix::fs::Mode {
        use rustix::fs::Mode;
        let owner = Mode::RUSR | Mode::WUSR;
        if visibility == Visibility::Public {
            owner | Mode::RGRP | Mode::ROTH
        } else {
            owner
        }
    }

    #[cfg(unix)]
    fn local_regular_single_link(fd: &std::os::fd::OwnedFd) -> Result<rustix::fs::Stat> {
        let stat = rustix::fs::fstat(fd).map_err(Error::internal)?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
            || stat.st_nlink != 1
        {
            return Err(Error::custom(
                "storage_unsafe_file",
                "storage file must be regular and have one hard link",
            ));
        }
        Ok(stat)
    }

    #[cfg(unix)]
    fn open_local_file(parent: &std::os::fd::OwnedFd, name: &str) -> Result<std::os::fd::OwnedFd> {
        let fd = rustix::fs::openat(
            parent,
            name,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(Error::internal)?;
        local_regular_single_link(&fd)?;
        Ok(fd)
    }

    #[cfg(unix)]
    fn publish_local(
        staged: &std::os::fd::OwnedFd,
        staging_parent: &std::os::fd::OwnedFd,
        staging_name: &str,
        target_parent: &std::os::fd::OwnedFd,
        target_name: &str,
        visibility: Visibility,
        cancellation: &UploadCancellation,
    ) -> Result<()> {
        publish_local_with_hook(
            staged,
            staging_parent,
            staging_name,
            target_parent,
            target_name,
            visibility,
            cancellation,
            || {},
            || {},
        )
    }

    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    fn publish_local_with_hook(
        staged: &std::os::fd::OwnedFd,
        staging_parent: &std::os::fd::OwnedFd,
        staging_name: &str,
        target_parent: &std::os::fd::OwnedFd,
        target_name: &str,
        visibility: Visibility,
        cancellation: &UploadCancellation,
        before_claim: impl FnOnce(),
        after_claim: impl FnOnce(),
    ) -> Result<()> {
        rustix::fs::fchmod(staged, local_file_mode(visibility)).map_err(Error::internal)?;
        before_claim();
        // CAS 决定取消与发布先后；一旦发布赢，守卫析构不再撤销同步 rename。
        let publication = cancellation.begin_publication()?;
        after_claim();
        let result = rustix::fs::renameat(staging_parent, staging_name, target_parent, target_name)
            .map_err(Error::internal);
        if result.is_ok() {
            publication.complete();
        } else {
            publication.fail();
        }
        result
    }

    #[cfg(all(test, unix))]
    #[test]
    fn cancellation_before_local_rename_preserves_existing_target() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().map_err(Error::internal)?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))
            .map_err(Error::internal)?;
        std::fs::create_dir(root.path().join("stage")).map_err(Error::internal)?;
        std::fs::set_permissions(
            root.path().join("stage"),
            std::fs::Permissions::from_mode(0o700),
        )
        .map_err(Error::internal)?;
        std::fs::write(root.path().join("stage/new"), b"new").map_err(Error::internal)?;
        std::fs::write(root.path().join("target"), b"old").map_err(Error::internal)?;
        let (stage_parent, stage_name) =
            local_parent(root.path(), "stage/new", Visibility::Private, false)?;
        let staged = open_local_file(&stage_parent, &stage_name)?;
        let (target_parent, target_name) =
            local_parent(root.path(), "target", Visibility::Public, false)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let publication = Arc::new(AtomicU8::new(UPLOAD_PENDING));
        let guard = UploadCancellationGuard {
            sender: Some(sender),
            publication: Arc::clone(&publication),
        };
        let cancellation = UploadCancellation {
            receiver,
            publication,
        };
        drop(guard);
        let result = publish_local(
            &staged,
            &stage_parent,
            &stage_name,
            &target_parent,
            &target_name,
            Visibility::Public,
            &cancellation,
        );
        assert_eq!(
            result.err().map(|error| error.code().to_owned()),
            Some("storage_upload_cancelled".to_owned())
        );
        assert_eq!(
            std::fs::read(root.path().join("target")).map_err(Error::internal)?,
            b"old"
        );
        assert!(root.path().join("stage/new").is_file());
        Ok(())
    }

    #[cfg(all(test, unix))]
    #[test]
    fn cancellation_during_local_publish_claim_preserves_existing_target() -> Result<()> {
        use std::sync::Barrier;
        let root = tempfile::tempdir().map_err(Error::internal)?;
        std::fs::create_dir(root.path().join("stage")).map_err(Error::internal)?;
        std::fs::write(root.path().join("stage/new"), b"new").map_err(Error::internal)?;
        std::fs::write(root.path().join("target"), b"old").map_err(Error::internal)?;
        let (stage_parent, stage_name) =
            local_parent(root.path(), "stage/new", Visibility::Private, false)?;
        let staged = open_local_file(&stage_parent, &stage_name)?;
        let (target_parent, target_name) =
            local_parent(root.path(), "target", Visibility::Private, false)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let publication = Arc::new(AtomicU8::new(UPLOAD_PENDING));
        let guard = UploadCancellationGuard {
            sender: Some(sender),
            publication: Arc::clone(&publication),
        };
        let cancellation = UploadCancellation {
            receiver,
            publication,
        };
        let reached = Arc::new(Barrier::new(2));
        let released = Arc::new(Barrier::new(2));
        let result = std::thread::scope(|scope| {
            let reached_worker = Arc::clone(&reached);
            let released_worker = Arc::clone(&released);
            let staged_ref = &staged;
            let stage_parent_ref = &stage_parent;
            let stage_name_ref = &stage_name;
            let target_parent_ref = &target_parent;
            let target_name_ref = &target_name;
            let worker = scope.spawn(move || {
                publish_local_with_hook(
                    staged_ref,
                    stage_parent_ref,
                    stage_name_ref,
                    target_parent_ref,
                    target_name_ref,
                    Visibility::Private,
                    &cancellation,
                    || {
                        reached_worker.wait();
                        released_worker.wait();
                    },
                    || {},
                )
            });
            reached.wait();
            drop(guard);
            released.wait();
            worker.join().map_err(|_| {
                Error::internal(std::io::Error::other("local publisher thread panicked"))
            })?
        });
        assert_eq!(
            result.err().map(|error| error.code().to_owned()),
            Some("storage_upload_cancelled".to_owned())
        );
        assert_eq!(
            std::fs::read(root.path().join("target")).map_err(Error::internal)?,
            b"old"
        );
        assert!(root.path().join("stage/new").is_file());
        Ok(())
    }

    #[cfg(all(test, unix))]
    #[test]
    fn local_publish_claim_survives_guard_drop_and_records_failure() -> Result<()> {
        use std::sync::Barrier;
        let root = tempfile::tempdir().map_err(Error::internal)?;
        std::fs::create_dir(root.path().join("stage")).map_err(Error::internal)?;
        std::fs::write(root.path().join("stage/new"), b"new").map_err(Error::internal)?;
        std::fs::write(root.path().join("target"), b"old").map_err(Error::internal)?;
        let (stage_parent, stage_name) =
            local_parent(root.path(), "stage/new", Visibility::Private, false)?;
        let staged = open_local_file(&stage_parent, &stage_name)?;
        let (target_parent, target_name) =
            local_parent(root.path(), "target", Visibility::Private, false)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let publication = Arc::new(AtomicU8::new(UPLOAD_PENDING));
        let guard = UploadCancellationGuard {
            sender: Some(sender),
            publication: Arc::clone(&publication),
        };
        let cancellation = UploadCancellation {
            receiver,
            publication: Arc::clone(&publication),
        };
        let reached = Arc::new(Barrier::new(2));
        let released = Arc::new(Barrier::new(2));
        let result = std::thread::scope(|scope| {
            let reached_worker = Arc::clone(&reached);
            let released_worker = Arc::clone(&released);
            let staged_ref = &staged;
            let stage_parent_ref = &stage_parent;
            let stage_name_ref = &stage_name;
            let target_parent_ref = &target_parent;
            let target_name_ref = &target_name;
            let worker = scope.spawn(move || {
                publish_local_with_hook(
                    staged_ref,
                    stage_parent_ref,
                    stage_name_ref,
                    target_parent_ref,
                    target_name_ref,
                    Visibility::Private,
                    &cancellation,
                    || {},
                    || {
                        reached_worker.wait();
                        released_worker.wait();
                    },
                )
            });
            reached.wait();
            drop(guard);
            released.wait();
            worker.join().map_err(|_| {
                Error::internal(std::io::Error::other("local publisher thread panicked"))
            })?
        });
        result?;
        assert_eq!(publication.load(Ordering::Acquire), UPLOAD_FINISHED);
        assert_eq!(
            std::fs::read(root.path().join("target")).map_err(Error::internal)?,
            b"new"
        );

        std::fs::write(root.path().join("stage/failed"), b"failed").map_err(Error::internal)?;
        let (stage_parent, stage_name) =
            local_parent(root.path(), "stage/failed", Visibility::Private, false)?;
        let staged = open_local_file(&stage_parent, &stage_name)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let publication = Arc::new(AtomicU8::new(UPLOAD_PENDING));
        let guard = UploadCancellationGuard {
            sender: Some(sender),
            publication: Arc::clone(&publication),
        };
        let cancellation = UploadCancellation {
            receiver,
            publication: Arc::clone(&publication),
        };
        assert!(
            publish_local(
                &staged,
                &stage_parent,
                &stage_name,
                &target_parent,
                "missing/target",
                Visibility::Private,
                &cancellation,
            )
            .is_err()
        );
        drop(guard);
        assert_eq!(publication.load(Ordering::Acquire), UPLOAD_FAILED);
        assert_eq!(
            std::fs::read(root.path().join("target")).map_err(Error::internal)?,
            b"new"
        );
        assert!(root.path().join("stage/failed").is_file());
        Ok(())
    }

    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    async fn write_local_object(
        operator: Operator,
        root: PathBuf,
        staging: PathBuf,
        path: String,
        content_type: Option<String>,
        requested_visibility: Option<Visibility>,
        body: StorageStream,
        mut cancellation: UploadCancellation,
    ) -> Result<StorageMetadata> {
        let visibility = requested_visibility.unwrap_or_default();
        let run_name = staging
            .file_name()
            .ok_or_else(|| Error::custom("storage_invalid_root", "staging path has no run name"))?;
        let key = format!(
            ".staging/{}/{}",
            run_name.to_string_lossy(),
            uuid::Uuid::now_v7()
        );
        let staged_path = root.join(&key);
        let result = async {
            let metadata =
                write_object(operator, key.clone(), content_type, body, &mut cancellation).await?;
            let (staging_parent, staging_name) =
                local_parent(&root, &key, Visibility::Private, false)?;
            let staged = open_local_file(&staging_parent, &staging_name)?;
            let (target_parent, target_name) = local_parent(&root, &path, visibility, true)?;
            publish_local(
                &staged,
                &staging_parent,
                &staging_name,
                &target_parent,
                &target_name,
                visibility,
                &cancellation,
            )?;
            Ok(metadata)
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&staged_path).await;
        }
        result
    }

    #[cfg(unix)]
    fn local_visibility(root: &std::path::Path, path: &StoragePath) -> Result<Visibility> {
        use rustix::fs::Mode;
        let (parent, name) = local_parent(root, path.as_str(), Visibility::Private, false)?;
        let file = open_local_file(&parent, &name)?;
        let stat = local_regular_single_link(&file)?;
        Ok(if Mode::from_raw_mode(stat.st_mode).contains(Mode::ROTH) {
            Visibility::Public
        } else {
            Visibility::Private
        })
    }

    #[cfg(unix)]
    fn set_local_visibility(
        root: &std::path::Path,
        path: &StoragePath,
        visibility: Visibility,
    ) -> Result<()> {
        let (parent, name) = local_parent(root, path.as_str(), visibility, false)?;
        let file = open_local_file(&parent, &name)?;
        rustix::fs::fchmod(&file, local_file_mode(visibility)).map_err(Error::internal)
    }

    #[cfg(not(unix))]
    #[allow(clippy::too_many_arguments)]
    async fn write_local_object(
        operator: Operator,
        _root: PathBuf,
        _staging: PathBuf,
        path: String,
        content_type: Option<String>,
        visibility: Option<Visibility>,
        body: StorageStream,
        mut cancellation: UploadCancellation,
    ) -> Result<StorageMetadata> {
        if visibility.is_some() {
            return Err(unsupported_storage_capability(
                "local permissions on this platform",
            ));
        }
        let publication = cancellation.begin_publication()?;
        let result = write_object(operator, path, content_type, body, &mut cancellation).await;
        if result.is_ok() {
            publication.complete();
        } else {
            publication.fail();
        }
        result
    }

    #[cfg(not(unix))]
    fn local_visibility(_root: &std::path::Path, _path: &StoragePath) -> Result<Visibility> {
        Err(unsupported_storage_capability(
            "local permissions on this platform",
        ))
    }

    #[cfg(not(unix))]
    fn set_local_visibility(
        _root: &std::path::Path,
        _path: &StoragePath,
        _visibility: Visibility,
    ) -> Result<()> {
        Err(unsupported_storage_capability(
            "local permissions on this platform",
        ))
    }

    #[async_trait]
    impl StorageBackend for OpenDalBackend {
        async fn put(
            &self,
            path: &StoragePath,
            content_type: Option<&str>,
            visibility: Option<Visibility>,
            body: StorageStream,
            cancellation: UploadCancellation,
        ) -> Result<StorageMetadata> {
            let operator = self.operator.clone();
            let path = path.as_str().to_owned();
            let content_type = content_type.map(str::to_owned);
            let local_root = self.local_root.clone();
            let staging_path = self.staging_path.clone();
            #[cfg(feature = "storage-s3")]
            let s3 = self.staging_path.is_none();
            #[cfg(feature = "storage-s3")]
            let s3_acl = self.s3_acl.clone();
            tokio::spawn(async move {
                #[cfg(feature = "storage-s3")]
                if s3 {
                    let acl =
                        s3_acl.ok_or_else(|| unsupported_storage_capability("S3 ACL client"))?;
                    return write_s3_object(
                        operator,
                        acl,
                        path,
                        content_type,
                        visibility,
                        body,
                        cancellation,
                    )
                    .await;
                }
                if let (Some(root), Some(staging)) = (local_root, staging_path) {
                    return write_local_object(
                        operator,
                        root,
                        staging,
                        path,
                        content_type,
                        visibility,
                        body,
                        cancellation,
                    )
                    .await;
                }
                Err(unsupported_storage_capability("local storage"))
            })
            .await
            .map_err(|_| {
                Error::custom(
                    "storage_upload_worker_failed",
                    "storage upload worker stopped unexpectedly",
                )
            })?
        }

        async fn visibility(&self, path: &StoragePath) -> Result<Visibility> {
            if let Some(root) = &self.local_root {
                let root = root.clone();
                let path = path.clone();
                return tokio::task::spawn_blocking(move || local_visibility(&root, &path))
                    .await
                    .map_err(Error::internal)?;
            }
            #[cfg(feature = "storage-s3")]
            if let Some(acl) = &self.s3_acl {
                return acl.visibility(path).await;
            }
            Err(unsupported_storage_capability("visibility reads"))
        }

        async fn set_visibility(&self, path: &StoragePath, visibility: Visibility) -> Result<()> {
            if let Some(root) = &self.local_root {
                let root = root.clone();
                let path = path.clone();
                return tokio::task::spawn_blocking(move || {
                    set_local_visibility(&root, &path, visibility)
                })
                .await
                .map_err(Error::internal)?;
            }
            #[cfg(feature = "storage-s3")]
            if let Some(acl) = &self.s3_acl {
                return acl.set_visibility(path, visibility).await;
            }
            Err(unsupported_storage_capability("visibility changes"))
        }

        async fn open(&self, path: &StoragePath, version: Option<&str>) -> Result<StorageReader> {
            let metadata = if let Some(version) = version {
                if !self.operator.info().capability().stat_with_version {
                    return Err(unsupported_storage_capability(
                        "version-specific metadata reads",
                    ));
                }
                self.operator
                    .stat_with(path.as_str())
                    .version(version)
                    .await
                    .map_err(map_opendal_error)?
            } else {
                self.operator
                    .stat(path.as_str())
                    .await
                    .map_err(map_opendal_error)?
            };
            let reader = self.operator.reader_with(path.as_str());
            let reader = if let Some(version) = version {
                if !self.operator.info().capability().read_with_version {
                    return Err(unsupported_storage_capability("version-specific reads"));
                }
                reader.version(version).await.map_err(map_opendal_error)?
            } else {
                reader.await.map_err(map_opendal_error)?
            };
            let body = reader
                .into_bytes_stream(..)
                .await
                .map_err(map_opendal_error)?
                .map(|chunk| {
                    chunk.map_err(|error| {
                        Error::custom(
                            "storage_backend_failed",
                            format!("storage stream read failed: {error}"),
                        )
                    })
                });
            Ok(StorageReader {
                metadata: metadata_from_opendal(metadata),
                body: Box::pin(body),
            })
        }

        async fn stat(&self, path: &StoragePath) -> Result<StorageMetadata> {
            self.operator
                .stat(path.as_str())
                .await
                .map(metadata_from_opendal)
                .map_err(map_opendal_error)
        }

        async fn delete(&self, path: &StoragePath) -> Result<()> {
            self.operator
                .delete(path.as_str())
                .await
                .map_err(map_opendal_error)
        }

        async fn presign_read(
            &self,
            path: &StoragePath,
            version: &str,
            expires_in: Duration,
        ) -> Result<String> {
            if !self.operator.info().capability().read_with_version {
                return Err(unsupported_storage_capability(
                    "version-bound presigned reads",
                ));
            }
            let request = self
                .operator
                .presign_read_options(
                    path.as_str(),
                    expires_in,
                    opendal::options::ReadOptions {
                        version: Some(version.to_owned()),
                        ..Default::default()
                    },
                )
                .await
                .map_err(map_opendal_error)?;
            Ok(request.uri().to_string())
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn metadata_from_opendal(metadata: opendal::Metadata) -> StorageMetadata {
        StorageMetadata {
            size: metadata.content_length(),
            content_type: metadata.content_type().map(str::to_owned),
            etag: metadata.etag().map(str::to_owned),
            version: metadata.version().map(str::to_owned),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn map_opendal_error(error: opendal::Error) -> Error {
        match error.kind() {
            opendal::ErrorKind::NotFound => {
                Error::custom("storage_object_not_found", "storage object was not found")
            }
            opendal::ErrorKind::Unsupported => {
                unsupported_storage_capability("requested operation")
            }
            _ => Error::custom("storage_backend_failed", "storage backend operation failed"),
        }
    }
}

#[cfg(feature = "storage-local")]
use opendal_driver::FsDriver;
#[cfg(feature = "storage-s3")]
use opendal_driver::S3Driver;

#[cfg(not(feature = "storage-local"))]
struct LocalUnavailableDriver;
#[cfg(not(feature = "storage-local"))]
#[async_trait]
impl StorageDriver for LocalUnavailableDriver {
    fn name(&self) -> &'static str {
        "fs"
    }
    fn validate(&self, _config: &StorageDriverConfig) -> Result<()> {
        Err(Error::custom(
            "storage_feature_required",
            "enable storage-local feature",
        ))
    }
    fn build<'a>(
        &'a self,
        _config: &'a StorageDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
        Box::pin(async {
            Err(Error::custom(
                "storage_feature_required",
                "enable the `storage-local` feature to use the filesystem driver",
            ))
        })
    }
}

#[cfg(not(feature = "storage-s3"))]
struct S3UnavailableDriver;
#[cfg(not(feature = "storage-s3"))]
#[async_trait]
impl StorageDriver for S3UnavailableDriver {
    fn name(&self) -> &'static str {
        "s3"
    }
    fn validate(&self, _config: &StorageDriverConfig) -> Result<()> {
        Err(Error::custom(
            "storage_feature_required",
            "enable storage-s3 feature",
        ))
    }
    fn build<'a>(
        &'a self,
        _config: &'a StorageDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
        Box::pin(async {
            Err(Error::custom(
                "storage_feature_required",
                "enable the `storage-s3` feature to use S3-compatible storage",
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::StoragePath;

    #[cfg(all(feature = "storage-local", unix))]
    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn local_visibility_applies_before_publication_and_preserves_private_parents()
    -> crate::Result<()> {
        use super::FilesystemManager;
        use super::FilesystemServiceProvider;
        use super::StorageStream;
        use super::UploadOptions;
        use super::Visibility;
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().map_err(crate::Error::internal)?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))
            .map_err(crate::Error::internal)?;
        let configuration = format!(
            "[storage.disks.local]\ndriver = 'fs'\nroot = '{}'\n[storage.disks.public]\ndriver = 'fs'\nroot = '{}'\nvisibility = 'public'\n",
            root.path().display(),
            root.path().display()
        );
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(FilesystemServiceProvider::new());
        let runtime = builder
            .start(figment::Figment::new().merge(Toml::string(&configuration)))
            .await?;
        let disk = runtime
            .container()
            .resolve::<FilesystemManager>()
            .await?
            .default_disk()
            .await?;
        let public_disk = runtime
            .container()
            .resolve::<FilesystemManager>()
            .await?
            .disk("public")
            .await?;
        assert_eq!(public_disk.default_visibility(), Some(Visibility::Public));
        let private = StoragePath::new("private/object.bin")?;
        disk.put_stream(
            private.clone(),
            None,
            Box::pin(stream::iter([Ok(bytes::Bytes::from_static(b"private"))])) as StorageStream,
            UploadOptions::default(),
            None,
        )
        .await?;
        assert_eq!(disk.visibility(&private).await?, Visibility::Private);
        assert_eq!(
            std::fs::metadata(root.path().join(private.as_str()))
                .map_err(crate::Error::internal)?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(
            disk.put_stream(
                private.clone(),
                None,
                Box::pin(stream::iter([Ok(bytes::Bytes::from_static(
                    b"replacement"
                ))])) as StorageStream,
                UploadOptions {
                    visibility: Some(Visibility::Public),
                    ..Default::default()
                },
                None,
            )
            .await
            .is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join(private.as_str())).map_err(crate::Error::internal)?,
            b"private"
        );
        let public = StoragePath::new("public/object.bin")?;
        disk.put_stream(
            public.clone(),
            None,
            Box::pin(stream::iter([Ok(bytes::Bytes::from_static(b"public"))])) as StorageStream,
            UploadOptions {
                visibility: Some(Visibility::Public),
                ..Default::default()
            },
            None,
        )
        .await?;
        assert_eq!(disk.visibility(&public).await?, Visibility::Public);
        assert_eq!(
            std::fs::metadata(root.path().join(public.as_str()))
                .map_err(crate::Error::internal)?
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
        assert_eq!(
            std::fs::metadata(root.path().join("public"))
                .map_err(crate::Error::internal)?
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        let named = StoragePath::new("named/object.bin")?;
        public_disk
            .put_stream(
                named.clone(),
                None,
                Box::pin(stream::iter([Ok(bytes::Bytes::from_static(b"named"))])) as StorageStream,
                UploadOptions::default(),
                None,
            )
            .await?;
        assert_eq!(public_disk.visibility(&named).await?, Visibility::Public);
        assert!(
            disk.set_visibility(&private, Visibility::Public)
                .await
                .is_err()
        );
        assert_eq!(disk.visibility(&private).await?, Visibility::Private);
        disk.set_visibility(&public, Visibility::Private).await?;
        assert_eq!(disk.visibility(&public).await?, Visibility::Private);
        std::fs::set_permissions(
            root.path().join("private"),
            std::fs::Permissions::from_mode(0o711),
        )
        .map_err(crate::Error::internal)?;
        disk.set_visibility(&private, Visibility::Public).await?;
        assert_eq!(disk.visibility(&private).await?, Visibility::Public);
        let external = tempfile::NamedTempFile::new().map_err(crate::Error::internal)?;
        std::fs::write(external.path(), b"external").map_err(crate::Error::internal)?;
        std::fs::set_permissions(external.path(), std::fs::Permissions::from_mode(0o600))
            .map_err(crate::Error::internal)?;
        std::fs::hard_link(external.path(), root.path().join("public/hardlink.bin"))
            .map_err(crate::Error::internal)?;
        let linked = StoragePath::new("public/hardlink.bin")?;
        assert_eq!(
            disk.set_visibility(&linked, Visibility::Public)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("storage_unsafe_file".to_owned())
        );
        assert_eq!(
            std::fs::read(external.path()).map_err(crate::Error::internal)?,
            b"external"
        );
        assert_eq!(
            std::fs::metadata(external.path())
                .map_err(crate::Error::internal)?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        symlink(root.path().join("public"), root.path().join("linked"))
            .map_err(crate::Error::internal)?;
        assert!(
            disk.visibility(&StoragePath::new("linked/object.bin")?)
                .await
                .is_err()
        );
        let replacement = tempfile::tempdir().map_err(crate::Error::internal)?;
        std::fs::write(replacement.path().join("object.bin"), b"outside")
            .map_err(crate::Error::internal)?;
        std::fs::rename(root.path().join("public"), root.path().join("moved-public"))
            .map_err(crate::Error::internal)?;
        symlink(replacement.path(), root.path().join("public")).map_err(crate::Error::internal)?;
        assert!(
            disk.set_visibility(&public, Visibility::Public)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(replacement.path().join("object.bin")).map_err(crate::Error::internal)?,
            b"outside"
        );
        Ok(())
    }

    #[cfg(feature = "storage-local")]
    use super::FilesystemManager;
    #[cfg(feature = "storage-local")]
    use super::FilesystemServiceProvider;
    #[cfg(feature = "storage-local")]
    use super::StorageStream;
    #[cfg(feature = "storage-local")]
    use super::UploadOptions;
    #[cfg(feature = "storage-local")]
    use crate::foundation::ProviderBuilder;
    #[cfg(feature = "storage-local")]
    use figment::providers::{Format, Toml};
    #[cfg(feature = "storage-local")]
    use futures_util::StreamExt;
    #[cfg(feature = "storage-local")]
    use futures_util::stream;
    #[cfg(feature = "storage-local")]
    use std::sync::Arc;
    #[cfg(feature = "storage-local")]
    use std::time::Duration;

    #[test]
    fn storage_path_rejects_absolute_and_traversal_keys() {
        for path in [
            "",
            "/etc/passwd",
            "../secret",
            "a/../secret",
            "a//b",
            "a\\b",
            "a\n",
            ".staging/run-1/temp",
            ".staging",
            ".openrust-staging/run-1/temp",
            ".openrust-staging",
        ] {
            assert!(StoragePath::new(path).is_err(), "accepted {path:?}");
        }
        assert_eq!(
            StoragePath::new("images/2026/avatar.png")
                .ok()
                .map(|path| path.as_str().to_owned())
                .as_deref(),
            Some("images/2026/avatar.png")
        );
    }

    #[cfg(feature = "storage-local")]
    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn local_disk_streams_objects_and_aborts_cancelled_uploads() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "openrust-storage-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(crate::Error::internal)?
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).map_err(crate::Error::internal)?;
        let root_value = root.to_string_lossy().replace('\\', "\\\\");
        let figment = figment::Figment::new().merge(Toml::string(&format!(
            "[storage]\nmax_upload_bytes = 8\nmax_chunk_bytes = 4\n[storage.disks.local]\ndriver = 'fs'\nroot = '{root_value}/objects'\n"
        )));
        let mut builder = ProviderBuilder::new();
        builder.provider(FilesystemServiceProvider::new());
        let app = builder.start(figment.clone()).await?;
        let disk = app
            .container()
            .resolve::<FilesystemManager>()
            .await?
            .default_disk()
            .await?;
        let mut second_builder = ProviderBuilder::new();
        second_builder.provider(FilesystemServiceProvider::new());
        let second_app = second_builder.start(figment).await?;
        let second_disk = second_app
            .container()
            .resolve::<FilesystemManager>()
            .await?
            .default_disk()
            .await?;
        let path = StoragePath::new("files/object.bin")?;
        let data = stream::iter([
            Ok(bytes::Bytes::from_static(b"abcd")),
            Ok(bytes::Bytes::from_static(b"ef")),
        ]);
        let stored = disk
            .put_stream(
                path.clone(),
                Some("application/octet-stream".into()),
                Box::pin(data) as StorageStream,
                UploadOptions::default(),
                None,
            )
            .await?;
        assert_eq!(stored.metadata.size, 6);
        let mut reader = disk.open(&path, None).await?;
        let mut output = Vec::new();
        while let Some(chunk) = reader.body.next().await {
            output.extend_from_slice(&chunk?);
        }
        assert_eq!(output, b"abcdef");

        for (tenant_id, bytes) in [("storage-a", b"A001"), ("storage-b", b"B001")] {
            crate::tenant::TenantContext::scope(crate::tenant::TenantId::new(tenant_id)?, async {
                let (writer, reader_disk) = if tenant_id == "storage-a" {
                    (disk.clone(), second_disk.clone())
                } else {
                    (second_disk.clone(), disk.clone())
                };
                writer
                    .put_stream(
                        path.clone(),
                        None,
                        Box::pin(stream::iter([Ok(bytes::Bytes::copy_from_slice(bytes))]))
                            as StorageStream,
                        UploadOptions::default(),
                        None,
                    )
                    .await?;
                let mut reader = reader_disk.open(&path, None).await?;
                let mut actual = Vec::new();
                while let Some(chunk) = reader.body.next().await {
                    actual.extend_from_slice(&chunk?);
                }
                assert_eq!(actual, bytes);
                Ok::<_, crate::Error>(())
            })
            .await??;
        }
        let mut unscoped = disk.open(&path, None).await?;
        let mut original = Vec::new();
        while let Some(chunk) = unscoped.body.next().await {
            original.extend_from_slice(&chunk?);
        }
        assert_eq!(original, b"abcdef");

        let rejected = disk
            .put_stream(
                path.clone(),
                None,
                Box::pin(stream::iter([
                    Ok(bytes::Bytes::from_static(b"new ")),
                    Err(crate::Error::custom("source_failed", "test stream failure")),
                ])) as StorageStream,
                UploadOptions::default(),
                None,
            )
            .await;
        assert!(rejected.is_err());
        let mut original = disk.open(&path, None).await?;
        let mut output = Vec::new();
        while let Some(chunk) = original.body.next().await {
            output.extend_from_slice(&chunk?);
        }
        assert_eq!(output, b"abcdef");

        let oversized_chunk_path = StoragePath::new("files/oversized-chunk.bin")?;
        let oversized_chunk = disk
            .put_stream(
                oversized_chunk_path.clone(),
                None,
                Box::pin(stream::iter([Ok(bytes::Bytes::from_static(b"12345"))])) as StorageStream,
                UploadOptions::default(),
                None,
            )
            .await;
        assert!(oversized_chunk.is_err());
        assert!(disk.stat(&oversized_chunk_path).await.is_err());

        let oversized_path = StoragePath::new("files/oversized.bin")?;
        let oversized = disk
            .put_stream(
                oversized_path.clone(),
                None,
                Box::pin(stream::iter([
                    Ok(bytes::Bytes::from_static(b"12345678")),
                    Ok(bytes::Bytes::from_static(b"9")),
                ])) as StorageStream,
                UploadOptions::default(),
                None,
            )
            .await;
        assert!(oversized.is_err());
        assert!(disk.stat(&oversized_path).await.is_err());

        let rejected_mime_path = StoragePath::new("files/rejected-mime.bin")?;
        let rejected_mime = disk
            .put_stream(
                rejected_mime_path.clone(),
                Some("text/plain".into()),
                Box::pin(stream::iter([Ok(bytes::Bytes::from_static(
                    b"not an image",
                ))])) as StorageStream,
                UploadOptions {
                    allowed_content_types: vec!["image/png".into()],
                    ..Default::default()
                },
                None,
            )
            .await;
        assert!(rejected_mime.is_err());
        assert!(disk.stat(&rejected_mime_path).await.is_err());

        let rejected_inspection_path = StoragePath::new("files/rejected-inspection.bin")?;
        let rejected_inspection = disk
            .put_stream(
                rejected_inspection_path.clone(),
                None,
                Box::pin(stream::iter([Ok(bytes::Bytes::from_static(b"blocked"))]))
                    as StorageStream,
                UploadOptions::default(),
                Some(Arc::new(RejectUpload)),
            )
            .await;
        assert!(rejected_inspection.is_err());
        assert!(disk.stat(&rejected_inspection_path).await.is_err());

        let staging_base = root.join("objects/.staging");
        let staging_objects = || -> crate::Result<usize> {
            let mut objects = 0;
            for run_directory in std::fs::read_dir(&staging_base).map_err(crate::Error::internal)? {
                let run_directory = run_directory.map_err(crate::Error::internal)?;
                if run_directory.file_name() == std::ffi::OsStr::new(".storage-owner") {
                    continue;
                }
                for object in
                    std::fs::read_dir(run_directory.path()).map_err(crate::Error::internal)?
                {
                    let _ = object.map_err(crate::Error::internal)?;
                    objects += 1;
                }
            }
            Ok(objects)
        };
        assert_eq!(staging_objects()?, 0);
        let cancelled_path = StoragePath::new("files/cancelled.bin")?;
        let (started, upload_started) = tokio::sync::oneshot::channel();
        let slow_stream = stream::once(async { Ok(bytes::Bytes::from_static(b"data")) }).chain(
            stream::once(async move {
                // 第二次拉取说明 writer 已接收首块，但尚未遇到 EOF 或发布目标。
                let _ = started.send(());
                std::future::pending::<crate::Result<bytes::Bytes>>().await
            }),
        );
        let upload = tokio::spawn({
            let disk = disk.clone();
            let cancelled_path = cancelled_path.clone();
            async move {
                disk.put_stream(
                    cancelled_path,
                    None,
                    Box::pin(slow_stream) as StorageStream,
                    UploadOptions::default(),
                    None,
                )
                .await
            }
        });
        tokio::time::timeout(Duration::from_secs(5), upload_started)
            .await
            .map_err(crate::Error::internal)?
            .map_err(crate::Error::internal)?;
        assert_eq!(staging_objects()?, 1);
        upload.abort();
        let cancelled = upload.await.err().ok_or_else(|| {
            crate::Error::custom("storage_test_upload_not_cancelled", "测试上传意外提前结束")
        })?;
        assert!(cancelled.is_cancelled());
        assert!(
            matches!(disk.stat(&cancelled_path).await, Err(error) if error.code() == "storage_object_not_found")
        );
        // abort 结束调用方，不等待独立上传 worker；检查异步清理的有界最终状态。
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if staging_objects()? == 0 {
                    return Ok::<(), crate::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            crate::Error::custom(
                "storage_test_staging_cleanup_timeout",
                "取消上传的 staging 对象未在期限内清理",
            )
        })??;
        assert!(
            matches!(disk.stat(&cancelled_path).await, Err(error) if error.code() == "storage_object_not_found")
        );
        for run_directory in std::fs::read_dir(&staging_base).map_err(crate::Error::internal)? {
            let run_directory = run_directory.map_err(crate::Error::internal)?;
            if run_directory.file_name() == std::ffi::OsStr::new(".storage-owner") {
                continue;
            }
            assert_eq!(
                std::fs::read_dir(run_directory.path())
                    .map_err(crate::Error::internal)?
                    .count(),
                0,
                "cancelled upload left a filesystem staging object"
            );
        }

        drop(second_app);
        std::fs::remove_dir_all(&root).map_err(crate::Error::internal)?;
        Ok(())
    }

    #[cfg(feature = "storage-local")]
    struct RejectUpload;

    #[cfg(feature = "storage-local")]
    #[async_trait::async_trait]
    impl super::UploadInspector for RejectUpload {
        async fn inspect_chunk(
            &self,
            _context: &super::UploadContext,
            _chunk: &[u8],
        ) -> crate::Result<()> {
            Err(crate::Error::Forbidden("测试检查器拒绝内容".into()))
        }
    }
}
