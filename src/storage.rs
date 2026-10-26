//! 文件存储、流式对象访问与公开 driver 扩展。
#![doc = include_str!("storage/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{
    collections::BTreeMap,
    fmt,
    pin::Pin,
    sync::{Arc, OnceLock},
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::de::DeserializeOwned;

use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};

pub use crate::config::{StorageConfig, StorageDiskConfig};

/// 存储读取或写入所使用的有序 bytes stream。
pub type StorageStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send + 'static>>;

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
    /// 写入 stream；只有成功完成时目标 key 才可见，失败/取消不能发布部分对象。
    async fn put(
        &self,
        path: &StoragePath,
        content_type: Option<&str>,
        body: StorageStream,
        cancellation: UploadCancellation,
    ) -> Result<StorageMetadata>;

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

/// 上传调用被丢弃时通知 driver 尽力清理未发布数据。
pub struct UploadCancellation {
    receiver: tokio::sync::oneshot::Receiver<()>,
}

impl UploadCancellation {
    /// 等待调用方取消；正常 EOF 不会触发该通知。
    pub async fn cancelled(&mut self) {
        let _ = (&mut self.receiver).await;
    }
}

struct UploadCancellationGuard(Option<tokio::sync::oneshot::Sender<()>>);

impl UploadCancellationGuard {
    fn complete(&mut self) {
        self.0.take();
    }
}

impl Drop for UploadCancellationGuard {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

/// 可从命名配置构造一个公开 storage backend。
pub trait StorageDriver: Send + Sync + 'static {
    /// driver 配置使用的稳定名称。
    fn name(&self) -> &'static str;

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
#[derive(Default)]
pub struct StorageDrivers {
    drivers: BTreeMap<&'static str, Arc<dyn StorageDriver>>,
}

impl StorageDrivers {
    /// 注册 driver；重复名称返回错误，不覆盖已有实现。
    pub fn register(&mut self, driver: impl StorageDriver) -> Result<()> {
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
        self.drivers.insert(name, Arc::new(driver));
        Ok(())
    }
}

#[derive(Default)]
struct StorageDisks {
    default_name: String,
    disks: BTreeMap<String, StorageDisk>,
}

/// 应用可查询 default 与 named storage disk 的统一服务。
#[derive(Clone, Default)]
pub struct Storage {
    disks: Arc<OnceLock<StorageDisks>>,
}

impl Storage {
    /// 返回配置选定的默认 disk。
    pub fn default_disk(&self) -> Result<StorageDisk> {
        let disks = self.disks.get().ok_or_else(storage_not_initialized)?;
        disks
            .disks
            .get(&disks.default_name)
            .cloned()
            .ok_or_else(|| {
                Error::custom(
                    "storage_default_disk_missing",
                    "default storage disk is missing",
                )
            })
    }

    /// 按名称取得 disk；未知名称不会回退到 default。
    pub fn disk(&self, name: &str) -> Result<StorageDisk> {
        self.disks
            .get()
            .ok_or_else(storage_not_initialized)?
            .disks
            .get(name)
            .cloned()
            .ok_or_else(|| {
                Error::custom(
                    "storage_disk_not_found",
                    format!("storage disk `{name}` is not configured"),
                )
            })
    }
}

/// 已初始化的具名 storage disk。
#[derive(Clone)]
pub struct StorageDisk {
    name: String,
    backend: Arc<dyn StorageBackend>,
    max_upload_bytes: u64,
    max_chunk_bytes: usize,
}

impl StorageDisk {
    /// 返回 disk 名称。
    pub fn name(&self) -> &str {
        &self.name
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
        let checked_body = checked_upload_stream(body, context.clone(), options, inspector);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let mut cancellation_guard = UploadCancellationGuard(Some(sender));
        let mut metadata = self
            .backend
            .put(
                &path,
                content_type.as_deref(),
                checked_body,
                UploadCancellation { receiver },
            )
            .await?;
        cancellation_guard.complete();
        if metadata.content_type.is_none() {
            metadata.content_type = content_type;
        }
        Ok(StoredObject { path, metadata })
    }

    /// 流式打开对象；可选的 version 必须由后端精确支持。
    pub async fn open(&self, path: &StoragePath, version: Option<&str>) -> Result<StorageReader> {
        let mut reader = self.backend.open(path, version).await?;
        reader.body = bounded_storage_stream(reader.body, self.max_chunk_bytes);
        Ok(reader)
    }

    /// 读取对象 metadata。
    pub async fn stat(&self, path: &StoragePath) -> Result<StorageMetadata> {
        self.backend.stat(path).await
    }

    /// 删除对象。
    pub async fn delete(&self, path: &StoragePath) -> Result<()> {
        self.backend.delete(path).await
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
        self.backend.presign_read(path, version, expires_in).await
    }
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

/// 初始化配置中的 storage disks。
#[derive(Debug, Default, Clone, Copy)]
pub struct StoragePlugin;

impl Plugin for StoragePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.try_insert(Storage::default())?;
        let drivers = app.extension_mut::<StorageDrivers>();
        #[cfg(feature = "storage-local")]
        drivers.register(FsDriver)?;
        #[cfg(not(feature = "storage-local"))]
        drivers.register(LocalUnavailableDriver)?;
        #[cfg(feature = "storage-s3")]
        drivers.register(S3Driver)?;
        #[cfg(not(feature = "storage-s3"))]
        drivers.register(S3UnavailableDriver)?;
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = match app.section::<StorageConfig>("storage") {
                Ok(config) => config,
                Err(crate::config::ConfigError::Source(source)) if source.missing() => {
                    StorageConfig::default()
                }
                Err(error) => return Err(Error::Config(error)),
            };
            validate_disk_name(&config.default_disk)?;
            if !config.disks.contains_key(&config.default_disk) {
                return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
                    "storage.default_disk `{}` is not configured",
                    config.default_disk
                ))));
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
            let drivers = app.extension::<StorageDrivers>().ok_or_else(|| {
                Error::custom(
                    "storage_driver_registry_missing",
                    "storage driver registry was not created",
                )
            })?;
            let mut disks = BTreeMap::new();
            for (name, disk) in config.disks {
                validate_disk_name(&name)?;
                let driver = drivers.drivers.get(disk.driver.as_str()).ok_or_else(|| {
                    Error::Config(crate::config::ConfigError::Invalid(format!(
                        "unknown storage driver `{}` for disk `{name}`",
                        disk.driver
                    )))
                })?;
                let driver_config = StorageDriverConfig {
                    disk_name: name.clone(),
                    options: disk.options,
                };
                let backend = driver.build(&driver_config).await?;
                disks.insert(
                    name.clone(),
                    StorageDisk {
                        name,
                        backend,
                        max_upload_bytes: config.max_upload_bytes,
                        max_chunk_bytes: config.max_chunk_bytes,
                    },
                );
            }
            let storage = app.state().try_get::<Storage>()?;
            storage
                .disks
                .set(StorageDisks {
                    default_name: config.default_disk,
                    disks,
                })
                .map_err(|_| {
                    Error::custom(
                        "storage_already_initialized",
                        "storage plugin initialized twice",
                    )
                })
        })
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

fn storage_not_initialized() -> Error {
    Error::custom(
        "storage_not_initialized",
        "storage service is not initialized",
    )
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
        StorageMetadata, StoragePath, StorageReader, StorageStream, UploadCancellation,
        async_trait, unsupported_storage_capability,
    };
    use futures_util::StreamExt;
    use opendal::{Operator, services};
    #[cfg(feature = "storage-s3")]
    use serde::de::DeserializeOwned;
    use std::path::PathBuf;
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
                let root = root.to_string_lossy().into_owned();
                let staging_base = PathBuf::from(&root).join(".openrust-staging");
                std::fs::create_dir_all(&staging_base).map_err(Error::internal)?;
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
                if !operator.info().capability().presign_read {
                    return Err(unsupported_storage_capability("presigned read"));
                }
                Ok(Arc::new(OpenDalBackend {
                    operator,
                    staging_path: None,
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
            let entry = entry.map_err(Error::internal)?;
            let path = entry.path();
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("run-")
                || !entry.file_type().map_err(Error::internal)?.is_dir()
            {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .map_err(Error::internal)?;
            let stale = modified < cutoff;
            if stale {
                std::fs::remove_dir_all(path).map_err(Error::internal)?;
            }
        }
        Ok(())
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
                Ok(()) => return Ok(path),
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

    async fn write_object(
        operator: Operator,
        path: String,
        content_type: Option<String>,
        mut body: StorageStream,
        mut cancellation: UploadCancellation,
    ) -> Result<StorageMetadata> {
        let options = opendal::options::WriteOptions {
            content_type,
            ..Default::default()
        };
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
                        () = cancellation.cancelled() => {
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

    #[async_trait]
    impl StorageBackend for OpenDalBackend {
        async fn put(
            &self,
            path: &StoragePath,
            content_type: Option<&str>,
            body: StorageStream,
            cancellation: UploadCancellation,
        ) -> Result<StorageMetadata> {
            let operator = self.operator.clone();
            let path = path.as_str().to_owned();
            let content_type = content_type.map(str::to_owned);
            tokio::spawn(async move {
                write_object(operator, path, content_type, body, cancellation).await
            })
            .await
            .map_err(|_| {
                Error::custom(
                    "storage_upload_worker_failed",
                    "storage upload worker stopped unexpectedly",
                )
            })?
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

    #[cfg(feature = "storage-local")]
    use super::{Storage, StoragePlugin, StorageStream, UploadOptions};
    #[cfg(feature = "storage-local")]
    use crate::core::{AppBuilder, lifecycle};
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
            ".openrust-staging/run-1/temp",
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
        let mut builder = AppBuilder::new(crate::config::FrameworkConfig::default(), figment);
        builder.plugin(StoragePlugin);
        let app = lifecycle::finalize(builder).await?;
        let disk = app.state().try_get::<Storage>()?.default_disk()?;
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

        let cancelled_path = StoragePath::new("files/cancelled.bin")?;
        let slow_stream = stream::unfold(0, |index| async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Some((Ok(bytes::Bytes::from_static(b"data")), index + 1))
        });
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
        tokio::time::sleep(Duration::from_millis(25)).await;
        upload.abort();
        let _ = upload.await;
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(disk.stat(&cancelled_path).await.is_err());
        let staging_base = root.join("objects/.openrust-staging");
        for run_directory in std::fs::read_dir(&staging_base).map_err(crate::Error::internal)? {
            let run_directory = run_directory.map_err(crate::Error::internal)?;
            assert_eq!(
                std::fs::read_dir(run_directory.path())
                    .map_err(crate::Error::internal)?
                    .count(),
                0,
                "cancelled upload left a filesystem staging object"
            );
        }

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
