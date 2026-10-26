#![doc = include_str!("cache/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod tags;
pub use tags::{CacheTagBackend, TaggedCacheEntry, TaggedCacheStore};

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    future::Future,
    path::PathBuf,
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};

pub use crate::config::cache::{
    CacheConfig, CacheSingleflightConfig, CacheStoreConfig, CacheSwrConfig,
};

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
    /// 返回可选的原子 tag generation 能力。
    fn tag_backend(&self) -> Option<&dyn CacheTagBackend> {
        None
    }
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

struct SingleFlight {
    flights: std::sync::Mutex<HashMap<String, Weak<Flight>>>,
    max_in_flight_keys: usize,
    max_waiters_per_key: usize,
    loader_timeout: Duration,
}

struct RefreshLimiter {
    active: AtomicUsize,
    maximum: usize,
}

struct RefreshPermit(Arc<RefreshLimiter>);

impl RefreshLimiter {
    fn acquire(self: &Arc<Self>) -> Option<RefreshPermit> {
        self.active
            .try_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum).then_some(active + 1)
            })
            .ok()
            .map(|_| RefreshPermit(Arc::clone(self)))
    }
}

impl Drop for RefreshPermit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Flight {
    state: tokio::sync::watch::Sender<FlightState>,
    waiters: AtomicUsize,
}

#[derive(Clone)]
enum FlightState {
    Running,
    Ready(Arc<[u8]>),
    Failed,
    Cancelled,
}

enum FlightRole {
    Leader(FlightGuard),
    Waiter(FlightWaiter),
}

struct FlightGuard {
    registry: Arc<SingleFlight>,
    key: String,
    flight: Arc<Flight>,
    active: bool,
}

struct FlightWaiter {
    receiver: tokio::sync::watch::Receiver<FlightState>,
    flight: Arc<Flight>,
}

impl SingleFlight {
    fn new(config: &CacheSingleflightConfig) -> Result<Arc<Self>> {
        if !(1..=4096).contains(&config.max_in_flight_keys)
            || !(1..=4096).contains(&config.max_waiters_per_key)
            || !(1..=300).contains(&config.loader_timeout_secs)
        {
            return Err(Error::Config(crate::config::ConfigError::Invalid(
                "cache.singleflight limits must be within their documented bounds".into(),
            )));
        }
        Ok(Arc::new(Self {
            flights: std::sync::Mutex::new(HashMap::new()),
            max_in_flight_keys: config.max_in_flight_keys,
            max_waiters_per_key: config.max_waiters_per_key,
            loader_timeout: Duration::from_secs(config.loader_timeout_secs),
        }))
    }

    fn begin(self: &Arc<Self>, key: String) -> Result<FlightRole> {
        let mut flights = self.flights.lock().map_err(|_| {
            Error::custom(
                "cache_singleflight_unavailable",
                "cache loader coordination is unavailable",
            )
        })?;
        flights.retain(|_, flight| flight.strong_count() > 0);
        if let Some(flight) = flights.get(&key).and_then(Weak::upgrade) {
            let receiver = flight.state.subscribe();
            let state = receiver.borrow().clone();
            if matches!(state, FlightState::Running) {
                let waiters = flight.waiters.fetch_add(1, Ordering::AcqRel);
                if waiters >= self.max_waiters_per_key {
                    flight.waiters.fetch_sub(1, Ordering::AcqRel);
                    return Err(Error::custom(
                        "cache_singleflight_waiter_limit",
                        "too many callers are waiting for this cache key",
                    ));
                }
                return Ok(FlightRole::Waiter(FlightWaiter { receiver, flight }));
            }
            flights.remove(&key);
        }
        if flights.len() >= self.max_in_flight_keys {
            return Err(Error::custom(
                "cache_singleflight_capacity",
                "too many cache loaders are already in flight",
            ));
        }
        let (state, _) = tokio::sync::watch::channel(FlightState::Running);
        let flight = Arc::new(Flight {
            state,
            waiters: AtomicUsize::new(0),
        });
        flights.insert(key.clone(), Arc::downgrade(&flight));
        Ok(FlightRole::Leader(FlightGuard {
            registry: Arc::clone(self),
            key,
            flight,
            active: true,
        }))
    }

    fn try_begin_refresh(self: &Arc<Self>, key: String) -> Result<Option<FlightGuard>> {
        let mut flights = self.flights.lock().map_err(|_| {
            Error::custom(
                "cache_singleflight_unavailable",
                "cache loader coordination is unavailable",
            )
        })?;
        flights.retain(|_, flight| flight.strong_count() > 0);
        if flights.get(&key).and_then(Weak::upgrade).is_some() {
            return Ok(None);
        }
        if flights.len() >= self.max_in_flight_keys {
            return Ok(None);
        }
        let (state, _) = tokio::sync::watch::channel(FlightState::Running);
        let flight = Arc::new(Flight {
            state,
            waiters: AtomicUsize::new(0),
        });
        flights.insert(key.clone(), Arc::downgrade(&flight));
        Ok(Some(FlightGuard {
            registry: Arc::clone(self),
            key,
            flight,
            active: true,
        }))
    }
}

impl FlightGuard {
    fn finish(mut self, state: FlightState) {
        self.flight.state.send_replace(state);
        self.remove();
        self.active = false;
    }

    fn remove(&self) {
        if let Ok(mut flights) = self.registry.flights.lock()
            && flights
                .get(&self.key)
                .and_then(Weak::upgrade)
                .is_some_and(|current| Arc::ptr_eq(&current, &self.flight))
        {
            flights.remove(&self.key);
        }
    }
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        if self.active {
            self.flight.state.send_replace(FlightState::Cancelled);
            self.remove();
        }
    }
}

impl FlightWaiter {
    async fn wait(mut self, timeout: Duration) -> Result<Option<Arc<[u8]>>> {
        let result = tokio::time::timeout(timeout, async {
            loop {
                let state = self.receiver.borrow().clone();
                match state {
                    FlightState::Running => {
                        if self.receiver.changed().await.is_err() {
                            return None;
                        }
                    }
                    FlightState::Ready(bytes) => return Some(Some(bytes)),
                    FlightState::Failed => {
                        return Some(None);
                    }
                    FlightState::Cancelled => return None,
                }
            }
        })
        .await;
        self.flight.waiters.fetch_sub(1, Ordering::AcqRel);
        match result {
            Err(_) => Err(Error::custom(
                "cache_loader_wait_timeout",
                "waiting for a cache loader exceeded its configured timeout",
            )),
            Ok(Some(Some(bytes))) => Ok(Some(bytes)),
            Ok(Some(None)) => Err(Error::custom(
                "cache_loader_failed",
                "the shared cache loader failed",
            )),
            Ok(None) => Ok(None),
        }
    }
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

    /// 缓存未命中时，在本进程合并同 key、同类型与同 load ID 的加载。
    pub async fn get_or_load_with<T, F, Fut>(
        &self,
        key: &str,
        load_id: &str,
        ttl: Option<Duration>,
        loader: F,
    ) -> Result<T>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T>> + Send,
    {
        self.default_store()?
            .get_or_load_with(key, load_id, ttl, loader)
            .await
    }
}

/// 已解析的缓存 store 句柄。
#[derive(Clone)]
pub struct CacheStore {
    store_name: String,
    namespace: String,
    backend: Arc<dyn CacheBackend>,
    singleflight: Arc<SingleFlight>,
    background_tasks: crate::core::lifecycle::BackgroundTaskHandle,
    refresh_limiter: Arc<RefreshLimiter>,
    shared_locks: Option<crate::db::SharedLocks>,
}

impl CacheStore {
    /// 为该 store 创建按 tag generation 隔离的视图。
    pub fn tagged(&self, tags: &[&str]) -> Result<TaggedCacheStore> {
        TaggedCacheStore::new(self.clone(), tags)
    }

    /// 递增一个或多个 tag 的 generation，使旧代条目不可见。
    pub async fn invalidate_tags(&self, tags: &[&str]) -> Result<()> {
        tags::invalidate_tags(self, tags).await
    }
    /// 返回后端实际共享范围。
    pub fn scope(&self) -> CacheScope {
        self.backend.scope()
    }

    /// 读取 JSON 值；未命中返回 `None`，错误不会转换为未命中。
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.get_at(&self.key(key)?).await
    }

    async fn get_at<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.backend
            .get(key)
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
        self.set_at(&self.key(key)?, value, ttl).await
    }

    async fn set_at<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let ttl = ttl.map(normalize_ttl).transpose()?;
        let bytes = serde_json::to_vec(value).map_err(|_| {
            Error::custom(
                "cache_serialization_failed",
                "cache value could not be encoded",
            )
        })?;
        self.backend.set(key, bytes, ttl).await
    }

    /// 删除 JSON 值。
    pub async fn remove(&self, key: &str) -> Result<()> {
        self.backend.remove(&self.key(key)?).await
    }

    /// 缓存未命中时，在本进程合并同 key、同类型与同 load ID 的加载。
    ///
    /// loader 可重复调用；leader 被取消后，等待者中的一个会重新竞争加载。
    /// 该保证仅限本进程，loader 的外部副作用仍需自行幂等。
    pub async fn get_or_load_with<T, F, Fut>(
        &self,
        key: &str,
        load_id: &str,
        ttl: Option<Duration>,
        loader: F,
    ) -> Result<T>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T>> + Send,
    {
        if key.is_empty() || key.len() > 1024 {
            return Err(Error::custom(
                "cache_invalid_key",
                "cache key must contain 1 to 1024 bytes",
            ));
        }
        validate_load_id(load_id)?;
        let ttl = ttl.map(normalize_ttl).transpose()?;
        let storage_key = self.key(key)?;
        if let Some(value) = self.get_at(&storage_key).await? {
            return Ok(value);
        }
        let flight_key = self.flight_key::<T>(key, load_id);
        loop {
            match self.singleflight.begin(flight_key.clone())? {
                FlightRole::Leader(guard) => {
                    if let Some(value) = self.get_at(&storage_key).await? {
                        let bytes = serde_json::to_vec(&value).map_err(|_| {
                            Error::custom(
                                "cache_serialization_failed",
                                "cache value could not be encoded",
                            )
                        });
                        return match bytes {
                            Ok(bytes) => {
                                guard.finish(FlightState::Ready(Arc::from(bytes)));
                                Ok(value)
                            }
                            Err(error) => {
                                guard.finish(FlightState::Failed);
                                Err(error)
                            }
                        };
                    }
                    let result = match tokio::time::timeout(
                        self.singleflight.loader_timeout,
                        loader(),
                    )
                    .await
                    {
                        Ok(Ok(value)) => value,
                        Ok(Err(error)) => {
                            guard.finish(FlightState::Failed);
                            return Err(error);
                        }
                        Err(_) => {
                            guard.finish(FlightState::Failed);
                            return Err(Error::custom(
                                "cache_loader_timeout",
                                "cache loader exceeded its configured timeout",
                            ));
                        }
                    };
                    let Ok(bytes) = serde_json::to_vec(&result) else {
                        guard.finish(FlightState::Failed);
                        return Err(Error::custom(
                            "cache_serialization_failed",
                            "cache value could not be encoded",
                        ));
                    };
                    if let Err(error) = self.set_at(&storage_key, &result, ttl).await {
                        guard.finish(FlightState::Failed);
                        return Err(error);
                    }
                    guard.finish(FlightState::Ready(Arc::from(bytes)));
                    return Ok(result);
                }
                FlightRole::Waiter(waiter) => {
                    if let Some(bytes) = waiter.wait(self.singleflight.loader_timeout).await? {
                        return serde_json::from_slice(&bytes).map_err(|_| {
                            Error::custom(
                                "cache_deserialization_failed",
                                "cached value could not be decoded",
                            )
                        });
                    }
                }
            }
        }
    }

    fn key(&self, key: &str) -> Result<String> {
        if key.is_empty() || key.len() > 1024 {
            return Err(Error::custom(
                "cache_invalid_key",
                "cache key must contain 1 to 1024 bytes",
            ));
        }
        let physical = if self.namespace.is_empty() {
            key.to_owned()
        } else {
            format!("{}:{key}", self.namespace)
        };
        if physical.starts_with("__openrust_cache_") {
            return Err(Error::custom(
                "cache_reserved_key",
                "cache key uses a reserved framework prefix",
            ));
        }
        Ok(physical)
    }

    fn flight_key<T>(&self, key: &str, load_id: &str) -> String {
        let type_name = std::any::type_name::<T>();
        format!(
            "{}:{}{}:{}{}:{}{}:{}{}:{}",
            self.store_name.len(),
            self.store_name,
            self.namespace.len(),
            self.namespace,
            key.len(),
            key,
            type_name.len(),
            type_name,
            load_id.len(),
            load_id
        )
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

fn validate_load_id(load_id: &str) -> Result<()> {
    if load_id.is_empty() || load_id.len() > 128 || load_id.chars().any(char::is_control) {
        return Err(Error::custom(
            "cache_invalid_load_id",
            "cache loader ID must contain 1 to 128 non-control bytes",
        ));
    }
    Ok(())
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
            let singleflight = SingleFlight::new(&config.singleflight)?;
            if !(1..=256).contains(&config.swr.max_in_flight_refreshes) {
                return Err(Error::Config(crate::config::ConfigError::Invalid(
                    "cache.swr.max_in_flight_refreshes must be within 1..=256".into(),
                )));
            }
            let background_tasks = app.state().background_tasks().handle();
            let refresh_limiter = Arc::new(RefreshLimiter {
                active: AtomicUsize::new(0),
                maximum: config.swr.max_in_flight_refreshes,
            });
            let shared_locks = app
                .state()
                .get::<crate::db::SharedLocks>()
                .map(|locks| (*locks).clone());
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
                    name.clone(),
                    CacheStore {
                        store_name: name.clone(),
                        namespace: driver_config.namespace,
                        backend,
                        singleflight: Arc::clone(&singleflight),
                        background_tasks: background_tasks.clone(),
                        refresh_limiter: Arc::clone(&refresh_limiter),
                        shared_locks: shared_locks.clone(),
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
    values: tokio::sync::Mutex<MemoryState>,
}

#[derive(Default)]
struct MemoryState {
    values: MemoryValues,
    generations: HashMap<String, u64>,
}
#[async_trait]
impl CacheBackend for MemoryBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut state = self.values.lock().await;
        if state
            .values
            .get(key)
            .is_some_and(|(_, expiry)| expiry.is_some_and(|time| time <= Instant::now()))
        {
            state.values.remove(key);
        }
        Ok(state.values.get(key).map(|(value, _)| value.clone()))
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
            .values
            .insert(key.to_owned(), (value, expires));
        Ok(())
    }
    async fn remove(&self, key: &str) -> Result<()> {
        self.values.lock().await.values.remove(key);
        Ok(())
    }
    fn tag_backend(&self) -> Option<&dyn CacheTagBackend> {
        Some(self)
    }
    fn scope(&self) -> CacheScope {
        CacheScope::Process
    }
}

#[async_trait]
impl CacheTagBackend for MemoryBackend {
    async fn generation(&self, tag_key: &str) -> Result<u64> {
        Ok(*self
            .values
            .lock()
            .await
            .generations
            .get(tag_key)
            .unwrap_or(&0))
    }

    async fn invalidate(&self, tag_key: &str) -> Result<()> {
        let mut state = self.values.lock().await;
        let generation = state.generations.entry(tag_key.to_owned()).or_default();
        *generation = generation.checked_add(1).ok_or_else(|| {
            Error::custom(
                "cache_tag_generation_exhausted",
                "cache tag generation is exhausted",
            )
        })?;
        Ok(())
    }

    async fn set_if_generations_match(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
        generations: &[(String, u64)],
    ) -> Result<bool> {
        let expires = Instant::now().checked_add(ttl).ok_or_else(|| {
            Error::custom("cache_invalid_ttl", "cache TTL exceeds supported range")
        })?;
        let mut state = self.values.lock().await;
        if generations
            .iter()
            .any(|(tag, expected)| state.generations.get(tag).copied().unwrap_or(0) != *expected)
        {
            return Ok(false);
        }
        state.values.insert(key.to_owned(), (value, Some(expires)));
        Ok(true)
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
    fn tag_backend(&self) -> Option<&dyn CacheTagBackend> {
        Some(self)
    }
    fn scope(&self) -> CacheScope {
        CacheScope::Shared
    }
}

#[cfg(feature = "cache-redis")]
#[async_trait]
impl CacheTagBackend for RedisBackend {
    async fn now_millis(&self) -> Result<u64> {
        let mut connection = self.connection.clone();
        let (seconds, microseconds): (u64, u64) = redis::cmd("TIME")
            .query_async(&mut connection)
            .await
            .map_err(|_| backend_failure())?;
        seconds
            .checked_mul(1000)
            .and_then(|millis| millis.checked_add(microseconds / 1000))
            .ok_or_else(backend_failure)
    }

    async fn generation(&self, tag_key: &str) -> Result<u64> {
        let mut connection = self.connection.clone();
        let value: Option<String> = redis::AsyncCommands::get(&mut connection, tag_key)
            .await
            .map_err(|_| backend_failure())?;
        value.map_or(Ok(0), |value| value.parse().map_err(|_| backend_failure()))
    }

    async fn invalidate(&self, tag_key: &str) -> Result<()> {
        let mut connection = self.connection.clone();
        let _: i64 = redis::AsyncCommands::incr(&mut connection, tag_key, 1_i64)
            .await
            .map_err(|_| backend_failure())?;
        Ok(())
    }

    async fn set_if_generations_match(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
        generations: &[(String, u64)],
    ) -> Result<bool> {
        const SCRIPT: &str = "local n = #KEYS - 1; for i = 2, #KEYS do local current = redis.call('GET', KEYS[i]); local expected = ARGV[i - 1]; if (not current and expected ~= '0') or (current and current ~= expected) then return 0 end end; redis.call('SET', KEYS[1], ARGV[n + 1], 'PX', ARGV[n + 2]); return 1";
        let mut connection = self.connection.clone();
        let mut command = redis::cmd("EVAL");
        command.arg(SCRIPT).arg(generations.len() + 1).arg(key);
        for (tag, _) in generations {
            command.arg(tag);
        }
        for (_, generation) in generations {
            command.arg(generation.to_string());
        }
        command.arg(value).arg(ttl.as_millis().to_string());
        let written: i32 = command
            .query_async(&mut connection)
            .await
            .map_err(|_| backend_failure())?;
        Ok(written == 1)
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
#[allow(clippy::expect_used, reason = "测试断言失败时立即给出故障上下文")]
mod tests {
    use super::*;
    use std::time::Duration;

    fn store(backend: Arc<dyn CacheBackend>, namespace: &str) -> CacheStore {
        store_with_config(backend, namespace, &CacheSingleflightConfig::default())
    }

    fn store_with_config(
        backend: Arc<dyn CacheBackend>,
        namespace: &str,
        config: &CacheSingleflightConfig,
    ) -> CacheStore {
        static TASKS: std::sync::OnceLock<crate::core::lifecycle::BackgroundTasks> =
            std::sync::OnceLock::new();
        let tasks = TASKS.get_or_init(crate::core::lifecycle::BackgroundTasks::default);
        store_with_runtime(backend, namespace, config, tasks)
    }

    fn store_with_runtime(
        backend: Arc<dyn CacheBackend>,
        namespace: &str,
        config: &CacheSingleflightConfig,
        tasks: &crate::core::lifecycle::BackgroundTasks,
    ) -> CacheStore {
        CacheStore {
            store_name: "test".to_owned(),
            backend,
            namespace: namespace.to_owned(),
            singleflight: SingleFlight::new(config).expect("valid singleflight test config"),
            background_tasks: tasks.handle(),
            refresh_limiter: Arc::new(RefreshLimiter {
                active: AtomicUsize::new(0),
                maximum: 64,
            }),
            shared_locks: None,
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
    async fn memory_tags_isolate_namespaces_and_reject_stale_generation_writes() -> Result<()> {
        let backend: Arc<dyn CacheBackend> = Arc::new(MemoryBackend::default());
        let cache_store = store(backend, "tenant-a");
        let tagged = cache_store.tagged(&["users"])?;
        tagged
            .set("profile:7", &"first", Duration::from_secs(30))
            .await?;
        assert_eq!(
            tagged.get::<String>("profile:7").await?,
            Some("first".into())
        );

        let old_entry = tagged.entry("profile:7").await?;
        cache_store.invalidate_tags(&["users"]).await?;
        assert_eq!(tagged.get::<String>("profile:7").await?, None);
        assert!(
            old_entry
                .set(&"stale", Duration::from_secs(30))
                .await
                .is_err()
        );

        let other_namespace = store(Arc::new(MemoryBackend::default()), "tenant-b");
        let other_tagged = other_namespace.tagged(&["users"])?;
        other_tagged
            .set("profile:7", &"other", Duration::from_secs(30))
            .await?;
        assert_eq!(
            other_tagged.get::<String>("profile:7").await?,
            Some("other".into())
        );
        assert!(cache_store.tagged(&[""]).is_err());
        let unnamespaced = CacheStore {
            store_name: "test".into(),
            namespace: String::new(),
            backend: Arc::clone(&cache_store.backend),
            singleflight: Arc::clone(&cache_store.singleflight),
            background_tasks: cache_store.background_tasks.clone(),
            refresh_limiter: Arc::clone(&cache_store.refresh_limiter),
            shared_locks: None,
        };
        assert!(
            unnamespaced
                .set("__openrust_cache_tag_meta_v1:reserved", &1, None)
                .await
                .is_err()
        );
        let file_store = store(
            Arc::new(FileBackend {
                root: PathBuf::from("."),
            }),
            "local",
        );
        assert!(file_store.tagged(&["users"]).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn singleflight_merges_loads_and_scopes_contract_keys() -> Result<()> {
        let cache = store(Arc::new(MemoryBackend::default()), "singleflight");
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(tokio::sync::Barrier::new(8));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let calls = Arc::clone(&calls);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                cache
                    .get_or_load_with("same", "profile-v1", Some(Duration::from_secs(5)), || {
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.fetch_add(1, Ordering::AcqRel);
                            tokio::time::sleep(Duration::from_millis(30)).await;
                            Ok(42_u32)
                        }
                    })
                    .await
            }));
        }
        for task in tasks {
            assert_eq!(task.await.map_err(Error::internal)??, 42);
        }
        assert_eq!(calls.load(Ordering::Acquire), 1);

        assert_ne!(
            cache.flight_key::<u32>("typed", "typed-v1"),
            cache.flight_key::<String>("typed", "typed-v1")
        );
        assert_ne!(
            cache.flight_key::<u32>("typed", "loader-a"),
            cache.flight_key::<u32>("typed", "loader-b")
        );

        let distinct_ids = Arc::new(AtomicUsize::new(0));
        let first_ids = Arc::clone(&distinct_ids);
        let second_ids = Arc::clone(&distinct_ids);
        let (first, second) = tokio::join!(
            cache.get_or_load_with("ids", "loader-a", None, move || {
                let ids = Arc::clone(&first_ids);
                async move {
                    ids.fetch_add(1, Ordering::AcqRel);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok(9_u8)
                }
            }),
            cache.get_or_load_with("ids", "loader-b", None, move || {
                let ids = Arc::clone(&second_ids);
                async move {
                    ids.fetch_add(1, Ordering::AcqRel);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok(9_u8)
                }
            })
        );
        assert_eq!(first?, 9);
        assert_eq!(second?, 9);
        assert_eq!(distinct_ids.load(Ordering::Acquire), 2);
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_singleflight_leader_releases_waiter_to_retry() -> Result<()> {
        let cache = store(Arc::new(MemoryBackend::default()), "cancelled");
        let entered = Arc::new(tokio::sync::Notify::new());
        let blocked = Arc::new(tokio::sync::Notify::new());
        let leader_cache = cache.clone();
        let leader_entered = Arc::clone(&entered);
        let leader_blocked = Arc::clone(&blocked);
        let leader = tokio::spawn(async move {
            leader_cache
                .get_or_load_with("key", "cancel-v1", None, move || {
                    let entered = Arc::clone(&leader_entered);
                    let blocked = Arc::clone(&leader_blocked);
                    async move {
                        entered.notify_one();
                        blocked.notified().await;
                        Ok(1_u8)
                    }
                })
                .await
        });
        entered.notified().await;

        let waiter_cache = cache.clone();
        let waiter = tokio::spawn(async move {
            waiter_cache
                .get_or_load_with("key", "cancel-v1", None, || async { Ok(2_u8) })
                .await
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        leader.abort();
        assert!(leader.await.is_err());
        assert_eq!(waiter.await.map_err(Error::internal)??, 2);
        assert_eq!(cache.get::<u8>("key").await?, Some(2));
        Ok(())
    }

    #[tokio::test]
    async fn singleflight_enforces_key_capacity_and_loader_timeout() -> Result<()> {
        let config = CacheSingleflightConfig {
            max_in_flight_keys: 1,
            max_waiters_per_key: 1,
            loader_timeout_secs: 1,
        };
        let cache = store_with_config(Arc::new(MemoryBackend::default()), "limits", &config);
        let entered = Arc::new(tokio::sync::Notify::new());
        let blocked = Arc::new(tokio::sync::Notify::new());
        let leader_cache = cache.clone();
        let leader_entered = Arc::clone(&entered);
        let leader_blocked = Arc::clone(&blocked);
        let leader = tokio::spawn(async move {
            leader_cache
                .get_or_load_with("first", "loader-v1", None, move || {
                    let entered = Arc::clone(&leader_entered);
                    let blocked = Arc::clone(&leader_blocked);
                    async move {
                        entered.notify_one();
                        blocked.notified().await;
                        Ok(1_u8)
                    }
                })
                .await
        });
        entered.notified().await;
        let capacity_error = cache
            .get_or_load_with("second", "loader-v1", None, || async { Ok(2_u8) })
            .await
            .expect_err("second key exceeds the in-flight key limit");
        assert_eq!(capacity_error.code(), "cache_singleflight_capacity");
        blocked.notify_one();
        assert_eq!(leader.await.map_err(Error::internal)??, 1);

        let timeout_error = cache
            .get_or_load_with("timeout", "loader-v1", None, || async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                Ok(3_u8)
            })
            .await
            .expect_err("loader exceeds configured timeout");
        assert_eq!(timeout_error.code(), "cache_loader_timeout");
        assert_eq!(
            cache
                .get_or_load_with("timeout", "loader-v1", None, || async { Ok(4_u8) })
                .await?,
            4
        );
        Ok(())
    }

    #[tokio::test]
    async fn singleflight_rejects_excess_waiters() -> Result<()> {
        let cache = store_with_config(
            Arc::new(MemoryBackend::default()),
            "waiter-limit",
            &CacheSingleflightConfig {
                max_in_flight_keys: 2,
                max_waiters_per_key: 1,
                loader_timeout_secs: 5,
            },
        );
        let entered = Arc::new(tokio::sync::Notify::new());
        let blocked = Arc::new(tokio::sync::Notify::new());
        let leader_cache = cache.clone();
        let leader_entered = Arc::clone(&entered);
        let leader_blocked = Arc::clone(&blocked);
        let leader = tokio::spawn(async move {
            leader_cache
                .get_or_load_with("key", "wait-v1", None, move || {
                    let entered = Arc::clone(&leader_entered);
                    let blocked = Arc::clone(&leader_blocked);
                    async move {
                        entered.notify_one();
                        blocked.notified().await;
                        Ok(1_u8)
                    }
                })
                .await
        });
        entered.notified().await;

        let waiter_cache = cache.clone();
        let waiter = tokio::spawn(async move {
            waiter_cache
                .get_or_load_with("key", "wait-v1", None, || async { Ok(2_u8) })
                .await
        });
        let flight_key = "test:waiter-limit:key:u8:wait-v1";
        for _ in 0..100 {
            let waiters = cache
                .singleflight
                .flights
                .lock()
                .expect("singleflight map lock")
                .get(flight_key)
                .and_then(Weak::upgrade)
                .map_or(0, |flight| flight.waiters.load(Ordering::Acquire));
            if waiters == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        let capacity_error = cache
            .get_or_load_with("key", "wait-v1", None, || async { Ok(3_u8) })
            .await
            .expect_err("second waiter exceeds limit");
        assert_eq!(capacity_error.code(), "cache_singleflight_waiter_limit");
        blocked.notify_one();
        assert_eq!(leader.await.map_err(Error::internal)??, 1);
        assert_eq!(waiter.await.map_err(Error::internal)??, 1);
        Ok(())
    }

    #[tokio::test]
    async fn singleflight_broadcasts_loader_failure_without_retrying_waiters() -> Result<()> {
        let cache = store(Arc::new(MemoryBackend::default()), "failure");
        let (leader, waiter) = tokio::join!(
            cache.get_or_load_with("key", "failure-v1", None, || async {
                tokio::time::sleep(Duration::from_millis(30)).await;
                Err::<u8, Error>(Error::custom("source_failed", "loader failed"))
            }),
            cache.get_or_load_with("key", "failure-v1", None, || async { Ok(1_u8) })
        );
        assert_eq!(
            leader.expect_err("leader returns its loader error").code(),
            "source_failed"
        );
        assert_eq!(
            waiter.expect_err("waiter receives shared failure").code(),
            "cache_loader_failed"
        );
        Ok(())
    }

    #[tokio::test]
    async fn swr_returns_stale_during_refresh_and_shares_the_refreshed_value() -> Result<()> {
        let tasks = crate::core::lifecycle::BackgroundTasks::default();
        let cache = store_with_runtime(
            Arc::new(MemoryBackend::default()),
            "swr",
            &CacheSingleflightConfig::default(),
            &tasks,
        );
        let tagged = cache.tagged(&["profiles"])?;
        let calls = Arc::new(AtomicUsize::new(0));
        let loader = {
            let calls = Arc::clone(&calls);
            move || {
                let calls = Arc::clone(&calls);
                async move { Ok(calls.fetch_add(1, Ordering::AcqRel) + 1) }
            }
        };
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "profile:1",
                    "profile-v1",
                    Duration::from_millis(50),
                    Duration::from_secs(2),
                    loader,
                )
                .await?,
            1
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        let stale = tagged
            .get_stale_while_revalidate(
                "profile:1",
                "profile-v1",
                Duration::from_millis(50),
                Duration::from_secs(2),
                {
                    let calls = Arc::clone(&calls);
                    move || {
                        let calls = Arc::clone(&calls);
                        async move { Ok(calls.fetch_add(1, Ordering::AcqRel) + 1) }
                    }
                },
            )
            .await?;
        assert_eq!(stale, 1);
        let mut refreshed = stale;
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(2)).await;
            refreshed = tagged
                .get_stale_while_revalidate(
                    "profile:1",
                    "profile-v1",
                    Duration::from_millis(50),
                    Duration::from_secs(2),
                    {
                        let calls = Arc::clone(&calls);
                        move || {
                            let calls = Arc::clone(&calls);
                            async move { Ok(calls.fetch_add(1, Ordering::AcqRel) + 1) }
                        }
                    },
                )
                .await?;
            if refreshed != stale {
                break;
            }
        }
        assert_eq!(refreshed, 2);
        assert_eq!(calls.load(Ordering::Acquire), 2);
        tasks.shutdown_default(Duration::from_secs(1)).await?;
        Ok(())
    }

    #[tokio::test]
    async fn swr_does_not_serve_stale_after_tags_are_invalidated() -> Result<()> {
        let tasks = crate::core::lifecycle::BackgroundTasks::default();
        let cache = store_with_runtime(
            Arc::new(MemoryBackend::default()),
            "swr-invalidation",
            &CacheSingleflightConfig::default(),
            &tasks,
        );
        let tagged = cache.tagged(&["profiles"])?;
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let completed = Arc::new(tokio::sync::Notify::new());
        let initial = {
            let calls = Arc::clone(&calls);
            move || {
                let call = calls.fetch_add(1, Ordering::AcqRel) + 1;
                async move { Ok(call) }
            }
        };
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "profile:1",
                    "profile-v1",
                    Duration::from_millis(20),
                    Duration::from_secs(2),
                    initial,
                )
                .await?,
            1
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        let stale = tagged
            .get_stale_while_revalidate(
                "profile:1",
                "profile-v1",
                Duration::from_millis(20),
                Duration::from_secs(2),
                {
                    let calls = Arc::clone(&calls);
                    let entered = Arc::clone(&entered);
                    let completed = Arc::clone(&completed);
                    move || {
                        let calls = Arc::clone(&calls);
                        let entered = Arc::clone(&entered);
                        let completed = Arc::clone(&completed);
                        async move {
                            let call = calls.fetch_add(1, Ordering::AcqRel) + 1;
                            entered.notify_one();
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            completed.notify_one();
                            Ok(call)
                        }
                    }
                },
            )
            .await?;
        assert_eq!(stale, 1);
        entered.notified().await;
        cache.invalidate_tags(&["profiles"]).await?;
        completed.notified().await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        let value = tagged
            .get_stale_while_revalidate(
                "profile:1",
                "profile-v1",
                Duration::from_secs(1),
                Duration::from_secs(1),
                {
                    let calls = Arc::clone(&calls);
                    move || {
                        let calls = Arc::clone(&calls);
                        async move { Ok(calls.fetch_add(1, Ordering::AcqRel) + 1) }
                    }
                },
            )
            .await?;
        assert_eq!(value, 3);
        tasks.shutdown_default(Duration::from_secs(1)).await?;
        Ok(())
    }

    #[tokio::test]
    async fn swr_refresh_failure_keeps_stale_only_until_the_maximum_window() -> Result<()> {
        let tasks = crate::core::lifecycle::BackgroundTasks::default();
        let cache = store_with_runtime(
            Arc::new(MemoryBackend::default()),
            "swr-expiry",
            &CacheSingleflightConfig::default(),
            &tasks,
        );
        let tagged = cache.tagged(&["records"])?;
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "record:1",
                    "record-v1",
                    Duration::from_millis(120),
                    Duration::from_millis(500),
                    || async { Ok(1_u8) },
                )
                .await?,
            1
        );
        tokio::time::sleep(Duration::from_millis(140)).await;
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "record:1",
                    "record-v1",
                    Duration::from_millis(120),
                    Duration::from_millis(500),
                    || async { Err::<u8, Error>(Error::custom("source_failed", "refresh failed")) },
                )
                .await?,
            1
        );
        tokio::time::sleep(Duration::from_millis(510)).await;
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "record:1",
                    "record-v1",
                    Duration::from_millis(120),
                    Duration::from_millis(500),
                    || async { Ok(3_u8) },
                )
                .await?,
            3
        );
        tasks.shutdown_default(Duration::from_secs(1)).await?;
        Ok(())
    }

    #[tokio::test]
    async fn swr_shutdown_cancels_and_joins_a_running_refresh() -> Result<()> {
        struct DropSignal(Arc<tokio::sync::Notify>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                self.0.notify_one();
            }
        }

        let tasks = crate::core::lifecycle::BackgroundTasks::default();
        let cache = store_with_runtime(
            Arc::new(MemoryBackend::default()),
            "swr-shutdown",
            &CacheSingleflightConfig::default(),
            &tasks,
        );
        let tagged = cache.tagged(&["records"])?;
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "record:1",
                    "record-v1",
                    Duration::from_millis(20),
                    Duration::from_secs(2),
                    || async { Ok(1_u8) },
                )
                .await?,
            1
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let dropped = Arc::new(tokio::sync::Notify::new());
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "record:1",
                    "record-v1",
                    Duration::from_millis(20),
                    Duration::from_secs(2),
                    {
                        let entered = Arc::clone(&entered);
                        let dropped = Arc::clone(&dropped);
                        move || {
                            let entered = Arc::clone(&entered);
                            let dropped = Arc::clone(&dropped);
                            async move {
                                let _drop_signal = DropSignal(dropped);
                                entered.notify_one();
                                std::future::pending::<Result<u8>>().await
                            }
                        }
                    },
                )
                .await?,
            1
        );
        entered.notified().await;
        tasks.shutdown_default(Duration::from_secs(1)).await?;
        dropped.notified().await;
        Ok(())
    }

    #[tokio::test]
    async fn swr_caps_concurrent_refreshes_across_keys() -> Result<()> {
        let tasks = crate::core::lifecycle::BackgroundTasks::default();
        let mut cache = store_with_runtime(
            Arc::new(MemoryBackend::default()),
            "swr-capacity",
            &CacheSingleflightConfig::default(),
            &tasks,
        );
        cache.refresh_limiter = Arc::new(RefreshLimiter {
            active: AtomicUsize::new(0),
            maximum: 1,
        });
        let tagged = cache.tagged(&["records"])?;
        for key in ["record:1", "record:2"] {
            tagged
                .get_stale_while_revalidate(
                    key,
                    "record-v1",
                    Duration::from_millis(20),
                    Duration::from_secs(2),
                    || async { Ok(1_u8) },
                )
                .await?;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let first_entered = Arc::clone(&entered);
        assert_eq!(
            tagged
                .get_stale_while_revalidate(
                    "record:1",
                    "record-v1",
                    Duration::from_millis(20),
                    Duration::from_secs(2),
                    move || {
                        let entered = Arc::clone(&first_entered);
                        async move {
                            entered.notify_one();
                            std::future::pending::<Result<u8>>().await
                        }
                    },
                )
                .await?,
            1
        );
        entered.notified().await;
        let second_calls = Arc::new(AtomicUsize::new(0));
        let second = tagged
            .get_stale_while_revalidate(
                "record:2",
                "record-v1",
                Duration::from_millis(20),
                Duration::from_secs(2),
                {
                    let second_calls = Arc::clone(&second_calls);
                    move || {
                        let second_calls = Arc::clone(&second_calls);
                        async move {
                            second_calls.fetch_add(1, Ordering::AcqRel);
                            Ok(2_u8)
                        }
                    }
                },
            )
            .await?;
        assert_eq!(second, 1);
        assert_eq!(second_calls.load(Ordering::Acquire), 0);
        tasks.shutdown_default(Duration::from_secs(1)).await?;
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

        let invalid_config = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault_store = 'primary'\n[cache.stores.primary]\ndriver = 'memory'\n[cache.singleflight]\nmax_in_flight_keys = 0\n",
        ));
        let mut invalid_builder =
            AppBuilder::new(crate::config::FrameworkConfig::default(), invalid_config);
        invalid_builder.plugin(CachePlugin);
        assert!(lifecycle::finalize(invalid_builder).await.is_err());

        let invalid_swr = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault_store = 'primary'\n[cache.stores.primary]\ndriver = 'memory'\n[cache.swr]\nmax_in_flight_refreshes = 0\n",
        ));
        let mut invalid_swr_builder =
            AppBuilder::new(crate::config::FrameworkConfig::default(), invalid_swr);
        invalid_swr_builder.plugin(CachePlugin);
        assert!(lifecycle::finalize(invalid_swr_builder).await.is_err());

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
            options: [("dsn".to_owned(), serde_json::Value::String(dsn.clone()))].into(),
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

        let config = CacheDriverConfig {
            store_name: "integration".into(),
            namespace: format!("cross-client:{}", std::process::id()),
            options: [("dsn".to_owned(), serde_json::Value::String(dsn))].into(),
        };
        let first_backend = RedisDriver.build(&config).await?;
        let second_backend = RedisDriver.build(&config).await?;
        let first = store(first_backend, &config.namespace).tagged(&["accounts"])?;
        let second = store(second_backend, &config.namespace);
        first
            .set("account:1", &"present", Duration::from_secs(30))
            .await?;
        let stale_entry = first.entry("account:2").await?;
        second.invalidate_tags(&["accounts"]).await?;
        assert_eq!(first.get::<String>("account:1").await?, None);
        assert!(
            stale_entry
                .set(&"late", Duration::from_secs(30))
                .await
                .is_err()
        );
        Ok(())
    }
}
