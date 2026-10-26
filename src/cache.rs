#![doc = include_str!("cache/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};

pub use crate::config::cache::{CacheConfig, CacheStoreConfig};

/// 缓存实例可见的共享范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheScope {
    /// 只在当前进程共享。
    Process,
    /// 当前节点上的进程可共享，其他节点不可见。
    NodeLocal,
    /// 多个连接该后端的节点可访问同一份数据。
    Shared,
}

/// 可由业务访问的缓存后端契约。值保持为 bytes，由 [`CacheStore`] 负责 JSON 编解码。
#[async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    /// 读取值；过期或不存在时返回 `None`。
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    /// 写入值；`None` 表示不过期，零时长由上层拒绝。
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()>;
    /// 删除值；键不存在时也成功。
    async fn remove(&self, key: &str) -> Result<()>;
    /// 返回此实例的实际共享范围。
    fn scope(&self) -> CacheScope;
}

/// 命名实例的已解析配置，供公开扩展 driver 构造后端。
#[derive(Clone)]
pub struct CacheDriverConfig {
    store_name: String,
    namespace: String,
    options: BTreeMap<String, serde_json::Value>,
}

impl CacheDriverConfig {
    /// 返回 store 名称。
    pub fn store_name(&self) -> &str {
        &self.store_name
    }
    /// 返回 key namespace。
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// 返回 driver 可解析的配置字段名称，不暴露字段值。
    pub fn option_names(&self) -> impl Iterator<Item = &str> {
        self.options.keys().map(String::as_str)
    }
    /// 读取 driver 自有配置项。
    pub fn option<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.options
            .get(key)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| {
                Error::custom(
                    "cache_invalid_driver_option",
                    "cache driver option has an invalid value",
                )
            })
    }
}

impl std::fmt::Debug for CacheDriverConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CacheDriverConfig")
            .field("store_name", &self.store_name)
            .field("namespace", &self.namespace)
            .field("option_names", &self.options.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// 根据命名配置创建一个缓存后端；外部 crate 可实现此 trait。
pub trait CacheDriver: Send + Sync + 'static {
    /// driver 配置中使用的稳定名称。
    fn name(&self) -> &'static str;
    /// 初始化后端；连接错误会使应用启动失败。
    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>>;
}

/// 由应用插件在组装阶段收集的缓存 driver 注册表。
#[derive(Default)]
pub struct CacheDrivers {
    drivers: BTreeMap<&'static str, Arc<dyn CacheDriver>>,
}

impl CacheDrivers {
    /// 注册一个 driver；重复名称返回错误，不会覆盖先前实现。
    pub fn register(&mut self, driver: impl CacheDriver) -> Result<()> {
        let name = driver.name();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(Error::custom(
                "cache_invalid_driver_name",
                "cache driver name must be lowercase ASCII",
            ));
        }
        if self.drivers.contains_key(name) {
            return Err(Error::custom(
                "cache_duplicate_driver",
                format!("cache driver `{name}` is already registered"),
            ));
        }
        self.drivers.insert(name, Arc::new(driver));
        Ok(())
    }
}

#[derive(Clone, Default)]
struct CacheStores {
    default_name: String,
    stores: BTreeMap<String, CacheStore>,
}

/// 可注入的默认缓存服务，可按名称取得其他 store。
#[derive(Clone, Default)]
pub struct Cache {
    stores: Arc<OnceLock<CacheStores>>,
}

impl Cache {
    /// 返回配置选定的默认 store。
    pub fn default_store(&self) -> Result<CacheStore> {
        let stores = self.stores.get().ok_or_else(not_initialized)?;
        stores
            .stores
            .get(&stores.default_name)
            .cloned()
            .ok_or_else(|| {
                Error::custom(
                    "cache_default_store_missing",
                    "configured default cache store is missing",
                )
            })
    }

    /// 按名称取得 store；未知名称返回错误，不回退到默认 store。
    pub fn store(&self, name: &str) -> Result<CacheStore> {
        self.stores
            .get()
            .ok_or_else(not_initialized)?
            .stores
            .get(name)
            .cloned()
            .ok_or_else(|| {
                Error::custom(
                    "cache_store_not_found",
                    format!("cache store `{name}` is not configured"),
                )
            })
    }

    /// 从默认 store 读取 JSON 值。
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.default_store()?.get(key).await
    }
    /// 将值写入默认 store。
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        self.default_store()?.set(key, value, ttl).await
    }
    /// 从默认 store 删除键。
    pub async fn remove(&self, key: &str) -> Result<()> {
        self.default_store()?.remove(key).await
    }
}

/// 已解析的缓存 store 句柄。
#[derive(Clone)]
pub struct CacheStore {
    namespace: String,
    backend: Arc<dyn CacheBackend>,
}

impl CacheStore {
    /// 返回后端实际共享范围。
    pub fn scope(&self) -> CacheScope {
        self.backend.scope()
    }

    /// 读取 JSON 值；未命中返回 `None`，错误不会转换为未命中。
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let key = self.key(key)?;
        self.backend
            .get(&key)
            .await?
            .map(|bytes| {
                serde_json::from_slice(&bytes).map_err(|_| {
                    Error::custom(
                        "cache_deserialization_failed",
                        "cached value could not be decoded",
                    )
                })
            })
            .transpose()
    }

    /// 写入 JSON 值，可选 TTL；TTL 为零或超出后端范围时返回错误。
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let key = self.key(key)?;
        let ttl = ttl.map(normalize_ttl).transpose()?;
        let bytes = serde_json::to_vec(value).map_err(|_| {
            Error::custom(
                "cache_serialization_failed",
                "cache value could not be encoded",
            )
        })?;
        self.backend.set(&key, bytes, ttl).await
    }

    /// 删除 JSON 值。
    pub async fn remove(&self, key: &str) -> Result<()> {
        self.backend.remove(&self.key(key)?).await
    }

    fn key(&self, key: &str) -> Result<String> {
        if key.is_empty() || key.len() > 1024 {
            return Err(Error::custom(
                "cache_invalid_key",
                "cache key must contain 1 to 1024 bytes",
            ));
        }
        Ok(if self.namespace.is_empty() {
            key.to_owned()
        } else {
            format!("{}:{key}", self.namespace)
        })
    }
}

fn normalize_ttl(duration: Duration) -> Result<Duration> {
    if duration.is_zero() {
        return Err(Error::custom(
            "cache_invalid_ttl",
            "cache TTL must be greater than zero",
        ));
    }
    let millis = duration.as_millis();
    let rounded = millis + u128::from(!duration.subsec_nanos().is_multiple_of(1_000_000));
    let millis = u64::try_from(rounded).map_err(|_| {
        Error::custom(
            "cache_invalid_ttl",
            "cache TTL must fit millisecond backend limits",
        )
    })?;
    Ok(Duration::from_millis(millis))
}

/// 初始化配置的默认与命名缓存实例。
#[derive(Debug, Default, Clone, Copy)]
pub struct CachePlugin;

impl Plugin for CachePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.try_insert(Cache::default())?;
        let registry = app.extension_mut::<CacheDrivers>();
        registry.register(MemoryDriver)?;
        registry.register(FileDriver)?;
        #[cfg(feature = "cache-redis")]
        registry.register(RedisDriver)?;
        #[cfg(not(feature = "cache-redis"))]
        registry.register(RedisUnavailableDriver)?;
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.section::<CacheConfig>("cache").map_err(Error::Config)?;
            validate_store_name(&config.default_store)?;
            if !config.stores.contains_key(&config.default_store) {
                return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
                    "cache.default_store `{}` is not configured",
                    config.default_store
                ))));
            }
            let drivers = app.extension::<CacheDrivers>().ok_or_else(|| {
                Error::custom(
                    "cache_driver_registry_missing",
                    "cache driver registry was not created",
                )
            })?;
            let mut stores = BTreeMap::new();
            for (name, store) in config.stores {
                validate_store_name(&name)?;
                let driver = drivers.drivers.get(store.driver.as_str()).ok_or_else(|| {
                    Error::Config(crate::config::ConfigError::Invalid(format!(
                        "unknown cache driver `{}` for store `{name}`",
                        store.driver
                    )))
                })?;
                let driver_config = CacheDriverConfig {
                    store_name: name.clone(),
                    namespace: store.namespace,
                    options: store.options,
                };
                let backend = driver.build(&driver_config).await?;
                stores.insert(
                    name,
                    CacheStore {
                        namespace: driver_config.namespace,
                        backend,
                    },
                );
            }
            let cache = app.state().try_get::<Cache>()?;
            cache
                .stores
                .set(CacheStores {
                    default_name: config.default_store,
                    stores,
                })
                .map_err(|_| {
                    Error::custom(
                        "cache_already_initialized",
                        "cache plugin was initialized more than once",
                    )
                })
        })
    }
}

fn validate_store_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
            "invalid cache store name `{name}`"
        ))));
    }
    Ok(())
}

fn not_initialized() -> Error {
    Error::custom("cache_not_initialized", "cache service is not initialized")
}
fn backend_failure() -> Error {
    Error::custom("cache_backend_error", "cache backend operation failed")
}

#[derive(Default)]
struct MemoryDriver;
impl CacheDriver for MemoryDriver {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async move {
            reject_unknown_options(config, &[])?;
            Ok(Arc::new(MemoryBackend::default()) as Arc<dyn CacheBackend>)
        })
    }
}

type MemoryValues = HashMap<String, (Vec<u8>, Option<Instant>)>;

#[derive(Default)]
struct MemoryBackend {
    values: tokio::sync::Mutex<MemoryValues>,
}
#[async_trait]
impl CacheBackend for MemoryBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut values = self.values.lock().await;
        if values
            .get(key)
            .is_some_and(|(_, expiry)| expiry.is_some_and(|time| time <= Instant::now()))
        {
            values.remove(key);
        }
        Ok(values.get(key).map(|(value, _)| value.clone()))
    }
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let expires = ttl
            .map(|duration| {
                Instant::now().checked_add(duration).ok_or_else(|| {
                    Error::custom("cache_invalid_ttl", "cache TTL exceeds supported range")
                })
            })
            .transpose()?;
        self.values
            .lock()
            .await
            .insert(key.to_owned(), (value, expires));
        Ok(())
    }
    async fn remove(&self, key: &str) -> Result<()> {
        self.values.lock().await.remove(key);
        Ok(())
    }
    fn scope(&self) -> CacheScope {
        CacheScope::Process
    }
}

#[derive(Default)]
struct FileDriver;
impl CacheDriver for FileDriver {
    fn name(&self) -> &'static str {
        "file"
    }
    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async move {
            reject_unknown_options(config, &["path"])?;
            let path: Option<PathBuf> = config.option("path")?;
            let root = path.ok_or_else(|| {
                Error::Config(crate::config::ConfigError::Invalid(format!(
                    "cache store `{}` using file driver requires `path`",
                    config.store_name
                )))
            })?;
            tokio::fs::create_dir_all(&root).await.map_err(|_| {
                Error::custom(
                    "cache_file_unavailable",
                    "file cache directory could not be created",
                )
            })?;
            Ok(Arc::new(FileBackend { root }) as Arc<dyn CacheBackend>)
        })
    }
}

struct FileBackend {
    root: PathBuf,
}
static FILE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[async_trait]
impl CacheBackend for FileBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.path(key);
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(backend_failure()),
        };
        if bytes.len() < 8 {
            return Err(backend_failure());
        }
        let expires = u64::from_be_bytes(bytes[..8].try_into().map_err(|_| backend_failure())?);
        if expires != 0 && expires <= unix_millis()? {
            let _ = tokio::fs::remove_file(path).await;
            return Ok(None);
        }
        Ok(Some(bytes[8..].to_vec()))
    }
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let expires = ttl
            .map(|duration| {
                unix_millis().and_then(|now| {
                    now.checked_add(
                        u64::try_from(duration.as_millis()).map_err(|_| backend_failure())?,
                    )
                    .ok_or_else(backend_failure)
                })
            })
            .transpose()?
            .unwrap_or(0);
        let mut bytes = expires.to_be_bytes().to_vec();
        bytes.extend(value);
        let sequence = FILE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = self
            .root
            .join(format!(".tmp-{}-{sequence}", std::process::id()));
        tokio::fs::write(&temp, bytes)
            .await
            .map_err(|_| backend_failure())?;
        if tokio::fs::rename(&temp, self.path(key)).await.is_err() {
            let _ = tokio::fs::remove_file(temp).await;
            return Err(backend_failure());
        }
        Ok(())
    }
    async fn remove(&self, key: &str) -> Result<()> {
        match tokio::fs::remove_file(self.path(key)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(backend_failure()),
        }
    }
    fn scope(&self) -> CacheScope {
        CacheScope::NodeLocal
    }
}

impl FileBackend {
    fn path(&self, key: &str) -> PathBuf {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(key.as_bytes());
        let mut name = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(name, "{byte:02x}");
        }
        self.root.join(name)
    }
}

fn unix_millis() -> Result<u64> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| backend_failure())?
        .as_millis();
    u64::try_from(value).map_err(|_| backend_failure())
}

fn reject_unknown_options(config: &CacheDriverConfig, allowed: &[&str]) -> Result<()> {
    if let Some(name) = config.option_names().find(|name| !allowed.contains(name)) {
        return Err(Error::Config(crate::config::ConfigError::Invalid(format!(
            "unknown option `{name}` for cache driver store `{}`",
            config.store_name
        ))));
    }
    Ok(())
}

#[cfg(feature = "cache-redis")]
#[derive(Default)]
struct RedisDriver;

#[cfg(feature = "cache-redis")]
impl CacheDriver for RedisDriver {
    fn name(&self) -> &'static str {
        "redis"
    }
    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async move {
            reject_unknown_options(config, &["dsn"])?;
            let dsn: Option<String> = config.option("dsn")?;
            let dsn = dsn.unwrap_or_else(|| "redis://127.0.0.1:6379/".to_owned());
            let client = redis::Client::open(dsn).map_err(|_| {
                Error::Config(crate::config::ConfigError::Invalid(format!(
                    "Redis configuration for cache store `{}` is invalid",
                    config.store_name
                )))
            })?;
            let connection = client.get_connection_manager().await.map_err(|_| {
                Error::Config(crate::config::ConfigError::Invalid(format!(
                    "Redis cache store `{}` is unavailable",
                    config.store_name
                )))
            })?;
            Ok(Arc::new(RedisBackend { connection }) as Arc<dyn CacheBackend>)
        })
    }
}

#[cfg(feature = "cache-redis")]
struct RedisBackend {
    connection: redis::aio::ConnectionManager,
}

#[cfg(feature = "cache-redis")]
#[async_trait]
impl CacheBackend for RedisBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut connection = self.connection.clone();
        redis::AsyncCommands::get(&mut connection, key)
            .await
            .map_err(|_| backend_failure())
    }
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let mut connection = self.connection.clone();
        if let Some(ttl) = ttl {
            let millis = u64::try_from(ttl.as_millis()).map_err(|_| {
                Error::custom("cache_invalid_ttl", "cache TTL exceeds Redis limits")
            })?;
            redis::cmd("SET")
                .arg(key)
                .arg(value)
                .arg("PX")
                .arg(millis)
                .query_async::<()>(&mut connection)
                .await
                .map_err(|_| backend_failure())
        } else {
            redis::AsyncCommands::set(&mut connection, key, value)
                .await
                .map_err(|_| backend_failure())
        }
    }
    async fn remove(&self, key: &str) -> Result<()> {
        let mut connection = self.connection.clone();
        redis::AsyncCommands::del(&mut connection, key)
            .await
            .map_err(|_| backend_failure())
    }
    fn scope(&self) -> CacheScope {
        CacheScope::Shared
    }
}

#[cfg(not(feature = "cache-redis"))]
struct RedisUnavailableDriver;
#[cfg(not(feature = "cache-redis"))]
impl CacheDriver for RedisUnavailableDriver {
    fn name(&self) -> &'static str {
        "redis"
    }
    fn build<'a>(
        &'a self,
        _config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async {
            Err(Error::Config(crate::config::ConfigError::Invalid(
                "cache driver `redis` requires the `cache-redis` feature".into(),
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn store(backend: Arc<dyn CacheBackend>, namespace: &str) -> CacheStore {
        CacheStore {
            backend,
            namespace: namespace.to_owned(),
        }
    }

    #[tokio::test]
    async fn memory_store_roundtrips_namespaces_ttl_and_removal() -> Result<()> {
        let backend: Arc<dyn CacheBackend> = Arc::new(MemoryBackend::default());
        let first = store(Arc::clone(&backend), "one");
        let second = store(backend, "two");
        first.set("item", &vec![1, 2], None).await?;
        assert_eq!(first.get::<Vec<u8>>("item").await?, Some(vec![1, 2]));
        assert_eq!(second.get::<Vec<u8>>("item").await?, None);
        first
            .set("short", &7_u8, Some(Duration::from_millis(1)))
            .await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(first.get::<u8>("short").await?, None);
        first.remove("item").await?;
        assert_eq!(first.get::<Vec<u8>>("item").await?, None);
        assert!(first.set("bad", &1, Some(Duration::ZERO)).await.is_err());
        first
            .set("short-nanos", &1, Some(Duration::from_nanos(1)))
            .await?;
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert_eq!(first.get::<u8>("short-nanos").await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn file_store_replaces_values_expires_and_uses_hashed_names() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "openrust-cache-{}-{}",
            std::process::id(),
            FILE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(Error::internal)?;
        let backend: Arc<dyn CacheBackend> = Arc::new(FileBackend { root: root.clone() });
        let cache = store(backend, "file");
        cache.set("../secret", &"one", None).await?;
        cache.set("../secret", &"two", None).await?;
        assert_eq!(
            cache.get::<String>("../secret").await?,
            Some("two".to_owned())
        );
        assert_eq!(
            std::fs::read_dir(&root).map_err(Error::internal)?.count(),
            1
        );
        cache
            .set("expired", &true, Some(Duration::from_millis(1)))
            .await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(cache.get::<bool>("expired").await?, None);
        tokio::fs::remove_dir_all(root)
            .await
            .map_err(Error::internal)?;
        Ok(())
    }

    #[tokio::test]
    async fn file_store_concurrent_replacements_leave_a_complete_value() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "openrust-cache-concurrent-{}-{}",
            std::process::id(),
            FILE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(Error::internal)?;
        let cache = store(Arc::new(FileBackend { root: root.clone() }), "parallel");
        let mut writers = Vec::new();
        for value in 0..16_u32 {
            let cache = cache.clone();
            writers.push(tokio::spawn(async move {
                cache.set("same-key", &value, None).await
            }));
        }
        for writer in writers {
            writer.await.map_err(Error::internal)??;
        }
        assert!(cache.get::<u32>("same-key").await?.is_some());
        assert_eq!(
            std::fs::read_dir(&root).map_err(Error::internal)?.count(),
            1
        );
        tokio::fs::remove_dir_all(root)
            .await
            .map_err(Error::internal)?;
        Ok(())
    }

    #[tokio::test]
    async fn plugin_requires_config_and_registers_named_memory_stores() -> Result<()> {
        use crate::core::{AppBuilder, lifecycle};
        use figment::providers::{Format, Toml};

        let mut missing_builder = AppBuilder::new(
            crate::config::FrameworkConfig::default(),
            figment::Figment::new(),
        );
        missing_builder.plugin(CachePlugin);
        let missing = lifecycle::finalize(missing_builder).await;
        assert!(missing.is_err());

        let figment = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault_store = 'primary'\n[cache.stores.primary]\ndriver = 'memory'\nnamespace = 'main'\n[cache.stores.secondary]\ndriver = 'memory'\nnamespace = 'other'\n",
        ));
        let mut builder = AppBuilder::new(crate::config::FrameworkConfig::default(), figment);
        builder.plugin(CachePlugin);
        let app = lifecycle::finalize(builder).await?;
        let cache = app.state().try_get::<Cache>()?;
        assert_eq!(cache.default_store()?.scope(), CacheScope::Process);
        assert_eq!(cache.store("secondary")?.scope(), CacheScope::Process);
        assert!(cache.store("unknown").is_err());
        cache.set("key", &"value", None).await?;
        assert_eq!(cache.get::<String>("key").await?, Some("value".into()));
        Ok(())
    }

    struct CustomDriverPlugin;
    impl Plugin for CustomDriverPlugin {
        fn build(&self, app: &mut AppBuilder) -> Result<()> {
            app.extension_mut::<CacheDrivers>().register(CustomDriver)
        }
    }

    struct CustomDriver;
    impl CacheDriver for CustomDriver {
        fn name(&self) -> &'static str {
            "custom-test"
        }
        fn build<'a>(
            &'a self,
            config: &'a CacheDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
            Box::pin(async move {
                let _setting: String = config
                    .option("setting")?
                    .ok_or_else(|| Error::custom("test_setting_missing", "test setting missing"))?;
                Ok(Arc::new(MemoryBackend::default()) as Arc<dyn CacheBackend>)
            })
        }
    }

    #[tokio::test]
    async fn external_driver_is_collected_before_cache_initialization() -> Result<()> {
        use crate::core::{AppBuilder, lifecycle};
        use figment::providers::{Format, Toml};

        let figment = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault_store = 'primary'\n[cache.stores.primary]\ndriver = 'custom-test'\nsetting = 'parsed'\n",
        ));
        let mut builder = AppBuilder::new(crate::config::FrameworkConfig::default(), figment);
        builder.plugin(CachePlugin).plugin(CustomDriverPlugin);
        let app = lifecycle::finalize(builder).await?;
        let cache = app.state().try_get::<Cache>()?;
        cache.set("key", &42, None).await?;
        assert_eq!(cache.get::<i32>("key").await?, Some(42));
        Ok(())
    }

    #[cfg(feature = "cache-redis")]
    #[tokio::test]
    async fn redis_connection_failure_does_not_fall_back_to_memory() {
        use crate::core::{AppBuilder, lifecycle};
        use figment::providers::{Format, Toml};

        let figment = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault_store = 'primary'\n[cache.stores.primary]\ndriver = 'redis'\ndsn = 'redis://127.0.0.1:1/'\n",
        ));
        let mut builder = AppBuilder::new(crate::config::FrameworkConfig::default(), figment);
        builder.plugin(CachePlugin);
        assert!(lifecycle::finalize(builder).await.is_err());
    }

    #[cfg(feature = "cache-redis")]
    #[tokio::test]
    async fn redis_store_roundtrips_ttl_on_live_service() -> Result<()> {
        let dsn =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_owned());
        let config = CacheDriverConfig {
            store_name: "integration".into(),
            namespace: format!("test:{}", std::process::id()),
            options: [("dsn".to_owned(), serde_json::Value::String(dsn))].into(),
        };
        let backend = RedisDriver.build(&config).await?;
        let cache = store(backend, "integration");
        cache.set("value", &"redis", None).await?;
        assert_eq!(cache.get::<String>("value").await?, Some("redis".into()));
        cache
            .set("expiring", &5, Some(Duration::from_millis(20)))
            .await?;
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(cache.get::<u8>("expiring").await?, None);
        cache.remove("value").await?;
        Ok(())
    }
}
