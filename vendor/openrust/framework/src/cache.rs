#![doc = include_str!("cache/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod memo;
#[cfg(feature = "cache-redis")]
mod redis_tags;
mod tags;
pub use tags::{CacheTagBackend, TaggedCacheEntry, TaggedCacheStore};

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::core::{Error, Result};
use crate::support::BoxFuture;

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
    /// 清空此后端的缓存数据。范围由后端决定，可能包含其他 key namespace。
    async fn flush(&self) -> Result<()> {
        Err(Error::custom(
            "cache_flush_unsupported",
            "cache backend does not support flushing",
        ))
    }
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
    /// 不执行 I/O 地校验 driver 配置；第三方 driver 可覆盖此方法。
    fn validate(&self, _config: &CacheDriverConfig) -> Result<()> {
        Ok(())
    }
    /// 初始化后端；连接错误会使应用启动失败。
    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>>;
}

/// 由应用插件在组装阶段收集的缓存 driver 注册表。
#[derive(Clone, Default)]
pub struct CacheDrivers {
    drivers: BTreeMap<&'static str, Arc<dyn CacheDriver>>,
}

impl CacheDrivers {
    /// 注册一个 driver；重复名称返回错误，不会覆盖先前实现。
    pub fn register(&mut self, driver: impl CacheDriver) -> Result<()> {
        self.register_shared(Arc::new(driver))
    }

    fn register_shared(&mut self, driver: Arc<dyn CacheDriver>) -> Result<()> {
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
        self.drivers.insert(name, driver);
        Ok(())
    }
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

struct CacheManager {
    resolver: crate::container::Resolver,
    drivers: std::sync::Mutex<CacheDrivers>,
    stores: std::sync::Mutex<BTreeMap<String, Arc<tokio::sync::OnceCell<CacheStore>>>>,
    runtime: std::sync::Mutex<Option<(Arc<SingleFlight>, Arc<RefreshLimiter>)>>,
    readiness: std::sync::Mutex<Option<crate::core::Readiness>>,
    readiness_names: std::sync::Mutex<HashSet<String>>,
}

/// 可注入的默认缓存服务，可按名称取得其他 store。
#[derive(Clone)]
pub struct Cache {
    manager: Arc<CacheManager>,
}

impl Cache {
    fn managed(resolver: crate::container::Resolver, drivers: CacheDrivers) -> Self {
        Self {
            manager: Arc::new(CacheManager {
                resolver,
                drivers: std::sync::Mutex::new(drivers),
                stores: std::sync::Mutex::new(BTreeMap::new()),
                runtime: std::sync::Mutex::new(None),
                readiness: std::sync::Mutex::new(None),
                readiness_names: std::sync::Mutex::new(HashSet::new()),
            }),
        }
    }

    /// 注册或替换 driver creator；已解析的 store 保持不变。
    pub fn extend(&self, driver: impl CacheDriver) -> Result<()> {
        let manager = &self.manager;
        let name = driver.name();
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::custom(
                "cache_invalid_driver_name",
                "cache driver name must be lowercase ASCII",
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

    /// 移除已解析 store，下一次读取时重新构造。
    pub fn forget_driver(&self, name: &str) -> Result<()> {
        let manager = &self.manager;
        let removed = manager
            .stores
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(name);
        drop(removed);
        Ok(())
    }

    /// 移除指定 store 的缓存引用。
    pub fn purge(&self, name: &str) -> Result<()> {
        self.forget_driver(name)
    }

    /// 校验当前配置与 driver 名称；driver 专属选项在构造时校验。
    pub fn validate_configured(&self) -> Result<()> {
        let manager = &self.manager;
        let config = manager.resolver.section::<CacheConfig>("cache")?;
        validate_cache_config(&config)?;
        let drivers = manager
            .drivers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for (name, store) in config.stores {
            let driver = drivers.drivers.get(store.driver.as_str()).ok_or_else(|| {
                Error::Config(crate::config::ConfigError::Invalid(
                    "unknown cache driver".into(),
                ))
            })?;
            driver.validate(&CacheDriverConfig {
                store_name: name,
                namespace: store.namespace,
                options: store.options,
            })?;
        }
        Ok(())
    }

    /// 返回当前配置选定的默认 store。
    pub async fn default_store(&self) -> Result<CacheStore> {
        self.store(&self.default_store_name()?).await
    }

    fn default_store_name(&self) -> Result<String> {
        Ok(self
            .manager
            .resolver
            .section::<CacheConfig>("cache")?
            .default)
    }

    /// 从当前容器作用域取得按 store 隔离的缓存快照。
    ///
    /// `None` 选择当前配置的默认 store。此方法与构造参数的 Cache 属性
    /// 共用一个作用域实例。读命中和未命中都保留到本作用域结束，
    /// 通过此实例修改键时会使对应的本地读取失效。
    pub async fn memo(
        resolver: &crate::container::Resolver,
        name: Option<&str>,
    ) -> Result<Arc<CacheStore>> {
        use crate::container::ServiceKey;

        let cache = resolver.resolve::<Self>().await?;
        let name = match name {
            Some(name) => name.to_owned(),
            None => cache.default_store_name()?,
        };
        validate_store_name(&name)?;
        let key = ServiceKey::named(format!("cache.__memoized:{name}"));
        resolver
            .container()?
            .scoped_key_if(key.clone(), move |_| {
                let name = name.clone();
                let cache = Arc::clone(&cache);
                Box::pin(async move {
                    let mut store = cache.store(&name).await?;
                    store.memo = Some(Arc::default());
                    Ok(Arc::new(store))
                })
            })
            .await?;
        resolver.resolve_key(key).await
    }

    /// 按名称异步构造并缓存 store；未知名称不会回退。
    pub async fn store(&self, name: &str) -> Result<CacheStore> {
        let manager = &self.manager;
        validate_store_name(name)?;
        let cell = manager
            .stores
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name.to_owned())
            .or_default()
            .clone();
        let value = cell
            .get_or_try_init(|| async {
                let config = manager.resolver.section::<CacheConfig>("cache")?;
                validate_cache_config(&config)?;
                let store = config.stores.get(name).ok_or_else(|| {
                    Error::custom("cache_store_not_found", "cache store is not configured")
                })?;
                let driver = manager
                    .drivers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .drivers
                    .get(store.driver.as_str())
                    .cloned()
                    .ok_or_else(|| {
                        Error::Config(crate::config::ConfigError::Invalid(
                            "unknown cache driver".into(),
                        ))
                    })?;
                let driver_config = CacheDriverConfig {
                    store_name: name.to_owned(),
                    namespace: store.namespace.clone(),
                    options: store.options.clone(),
                };
                let background_tasks = manager.resolver.resources()?.background_handle()?;
                let shared_locks = if manager
                    .resolver
                    .is_bound::<crate::database::SharedLocks>()?
                {
                    Some(
                        (*manager
                            .resolver
                            .resolve::<crate::database::SharedLocks>()
                            .await?)
                            .clone(),
                    )
                } else {
                    None
                };
                let (singleflight, refresh_limiter) = {
                    let mut runtime = manager
                        .runtime
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if runtime.is_none() {
                        *runtime = Some((
                            SingleFlight::new(&config.singleflight)?,
                            Arc::new(RefreshLimiter {
                                active: AtomicUsize::new(0),
                                maximum: config.swr.max_in_flight_refreshes,
                            }),
                        ));
                    }
                    runtime.as_ref().cloned().ok_or_else(not_initialized)?
                };
                driver.validate(&driver_config)?;
                let backend = driver.build(&driver_config).await?;
                Ok::<_, Error>(CacheStore {
                    store_name: name.to_owned(),
                    namespace: driver_config.namespace,
                    backend,
                    singleflight,
                    background_tasks,
                    refresh_limiter,
                    shared_locks,
                    memo: None,
                })
            })
            .await?;
        self.register_readiness_probe(name)?;
        Ok(value.clone())
    }

    fn install_readiness(&self, readiness: crate::core::Readiness) -> Result<()> {
        let manager = &self.manager;
        *manager
            .readiness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(readiness);
        let names: Vec<_> = manager
            .stores
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(_, cell)| cell.get().is_some())
            .map(|(name, _)| name.clone())
            .collect();
        for name in names {
            self.register_readiness_probe(&name)?;
        }
        Ok(())
    }

    fn register_readiness_probe(&self, name: &str) -> Result<()> {
        let manager = &self.manager;
        let Some(readiness) = manager
            .readiness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        else {
            return Ok(());
        };
        let mut names = manager
            .readiness_names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if names.contains(name) {
            return Ok(());
        }
        let manager = Arc::downgrade(manager);
        let store_name = name.to_owned();
        readiness.register_probe(format!("cache.{name}"), move || {
            let manager = manager.clone();
            let store_name = store_name.clone();
            async move {
                let Some(manager) = manager.upgrade() else {
                    return Ok(());
                };
                let cell = manager
                    .stores
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&store_name)
                    .cloned();
                if let Some(store) = cell.and_then(|cell| cell.get().cloned()) {
                    store.check_readiness().await?;
                }
                Ok(())
            }
        })?;
        names.insert(name.to_owned());
        Ok(())
    }

    /// 从默认 store 读取 JSON 值。
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.default_store().await?.get(key).await
    }
    /// 将值写入默认 store。
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        self.default_store().await?.set(key, value, ttl).await
    }
    /// 从默认 store 删除键。
    pub async fn remove(&self, key: &str) -> Result<()> {
        self.default_store().await?.remove(key).await
    }

    /// 清空默认 store 的缓存数据。Redis 会清空该连接选中的整个数据库。
    pub async fn flush(&self) -> Result<()> {
        self.default_store().await?.flush().await
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
        self.default_store()
            .await?
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
    background_tasks: crate::foundation::lifecycle::BackgroundTaskHandle,
    refresh_limiter: Arc<RefreshLimiter>,
    shared_locks: Option<crate::database::SharedLocks>,
    memo: Option<Arc<memo::MemoizedValues>>,
}

impl CacheStore {
    /// 为该 store 创建按 tag generation 隔离的视图。
    pub fn tagged(&self, tags: &[&str]) -> Result<TaggedCacheStore> {
        TaggedCacheStore::new(self.clone(), tags)
    }

    /// 失效一个或多个 tag 的旧代条目，Redis 同时删除已索引的值。
    pub async fn invalidate_tags(&self, tags: &[&str]) -> Result<()> {
        tags::invalidate_tags(self, tags).await
    }
    /// 返回后端实际共享范围。
    pub fn scope(&self) -> CacheScope {
        self.backend.scope()
    }

    async fn check_readiness(&self) -> Result<()> {
        let key = self.key_for_tenant("__readiness_probe__", None)?;
        self.backend.get(&key).await.map(|_| ())
    }

    /// 读取 JSON 值；未命中返回 `None`，错误不会转换为未命中。
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.get_at(&self.key(key)?).await
    }

    async fn get_at<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let value = match &self.memo {
            Some(memo) => memo.get(self.backend.as_ref(), key).await?,
            None => self.backend.get(key).await?,
        };
        Self::decode(value)
    }

    fn decode<T: DeserializeOwned>(value: Option<Vec<u8>>) -> Result<Option<T>> {
        value
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
        let _write = self.memo.as_ref().map(|memo| memo.begin_write(key));
        self.backend.set(key, bytes, ttl).await
    }

    /// 删除 JSON 值。
    pub async fn remove(&self, key: &str) -> Result<()> {
        self.remove_at(&self.key(key)?).await
    }

    /// 清空所选 store。多个 namespace 共用同一个 Redis 库或文件目录时会一并清理。
    pub async fn flush(&self) -> Result<()> {
        let _flush = self.memo.as_ref().map(|memo| memo.begin_flush());
        self.backend.flush().await
    }

    async fn remove_at(&self, key: &str) -> Result<()> {
        let _write = self.memo.as_ref().map(|memo| memo.begin_write(key));
        self.backend.remove(key).await
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
        let tenant =
            crate::tenant::TenantContext::current().map(|tenant| tenant.as_str().to_owned());
        self.key_for_tenant(key, tenant.as_deref())
    }

    fn key_for_tenant(&self, key: &str, tenant: Option<&str>) -> Result<String> {
        if key.is_empty() || key.len() > 1024 {
            return Err(Error::custom(
                "cache_invalid_key",
                "cache key must contain 1 to 1024 bytes",
            ));
        }
        let base = if self.namespace.is_empty() {
            key.to_owned()
        } else {
            format!("{}:{key}", self.namespace)
        };
        let physical = tenant.map_or(base.clone(), |tenant| {
            format!("__tenant_{}:{base}", cache_tenant_digest(tenant))
        });
        if key.starts_with("__cache_")
            || key.starts_with("__openrust_cache_")
            || key.starts_with("__tenant_")
            || key.starts_with("__openrust_tenant_")
            || physical.starts_with("__cache_")
            || physical.starts_with("__openrust_cache_")
            || (tenant.is_none() && physical.starts_with("__tenant_"))
            || physical.starts_with("__openrust_tenant_")
        {
            return Err(Error::custom(
                "cache_reserved_key",
                "cache key uses a reserved framework prefix",
            ));
        }
        Ok(physical)
    }

    fn flight_key<T>(&self, key: &str, load_id: &str) -> String {
        let type_name = std::any::type_name::<T>();
        let tenant = crate::tenant::TenantContext::current()
            .map(|tenant| cache_tenant_digest(tenant.as_str()))
            .unwrap_or_default();
        format!(
            "{}:{}{}:{}{}:{}{}:{}{}:{}{}:{}",
            self.store_name.len(),
            self.store_name,
            self.namespace.len(),
            self.namespace,
            tenant.len(),
            tenant,
            key.len(),
            key,
            type_name.len(),
            type_name,
            load_id.len(),
            load_id
        )
    }
}

fn cache_tenant_digest(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let bytes = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
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

/// 按当前配置装配内置及应用自定义缓存 driver。
///
/// Provider 尚待生产入口统一切换。
#[derive(Default)]
pub struct CacheServiceProvider {
    drivers: Vec<Arc<dyn CacheDriver>>,
}

impl CacheServiceProvider {
    /// 创建不含应用自定义 driver 的 Provider。
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加应用 driver；与既有名称冲突时启动失败。
    #[must_use]
    pub fn with_driver(mut self, driver: impl CacheDriver) -> Self {
        self.drivers.push(Arc::new(driver));
        self
    }
}

fn validate_cache_config(config: &CacheConfig) -> Result<()> {
    validate_store_name(&config.default)?;
    if !config.stores.contains_key(&config.default) {
        return Err(Error::Config(crate::config::ConfigError::Invalid(
            "cache.default is not configured".into(),
        )));
    }
    let _ = SingleFlight::new(&config.singleflight)?;
    if !(1..=256).contains(&config.swr.max_in_flight_refreshes) {
        return Err(Error::Config(crate::config::ConfigError::Invalid(
            "cache.swr.max_in_flight_refreshes must be within 1..=256".into(),
        )));
    }
    for name in config.stores.keys() {
        validate_store_name(name)?;
    }
    Ok(())
}

impl crate::support::ServiceProvider for CacheServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as S;
            app.config_section(
                "cache",
                CacheConfig::default(),
                S::object([
                    ("default", S::String),
                    (
                        "stores",
                        S::map(S::object([("driver", S::String), ("namespace", S::String)]).open()),
                    ),
                    (
                        "singleflight",
                        S::object([
                            ("max_in_flight_keys", S::Integer),
                            ("max_waiters_per_key", S::Integer),
                            ("loader_timeout_secs", S::Integer),
                        ]),
                    ),
                    ("swr", S::object([("max_in_flight_refreshes", S::Integer)])),
                ]),
                validate_cache_config,
            )?;
            let custom = self.drivers.clone();
            app.singleton::<Cache, _>(move |resolver| {
                let custom = custom.clone();
                Box::pin(async move {
                    let mut drivers = CacheDrivers::default();
                    drivers.register(MemoryDriver)?;
                    drivers.register(FileDriver)?;
                    #[cfg(feature = "cache-redis")]
                    drivers.register(RedisDriver)?;
                    #[cfg(not(feature = "cache-redis"))]
                    drivers.register(RedisUnavailableDriver)?;
                    for driver in custom {
                        drivers.register_shared(driver)?;
                    }
                    Ok(Arc::new(Cache::managed(resolver, drivers)))
                })
            })
            .await?;
            app.require::<Cache>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let cache = app.resolve::<Cache>().await?;
            let _ = cache.default_store().await?;
            let final_cache = Arc::clone(&cache);
            app.validate_at_finalize(move || {
                Box::pin(async move { final_cache.validate_configured() })
            });
            if app.is_bound::<crate::core::Readiness>() {
                let readiness = app.resolve::<crate::core::Readiness>().await?;
                cache.install_readiness((*readiness).clone())?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/cache/flush.rs"]
mod flush_tests;

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::foundation::BootContext;
    use crate::foundation::ProviderBuilder;
    use crate::support::ServiceProvider;
    use figment::{Figment, providers::Serialized};
    use serde_json::json;

    struct ReadinessProvider(Arc<crate::core::Readiness>);
    impl ServiceProvider for ReadinessProvider {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::clone(&self.0)).await?;
                Ok(())
            })
        }
    }

    struct ProbeBackend(bool);
    #[async_trait]
    impl CacheBackend for ProbeBackend {
        async fn get(&self, _: &str) -> Result<Option<Vec<u8>>> {
            if self.0 {
                Err(Error::custom("probe_failed", "probe failed"))
            } else {
                Ok(None)
            }
        }
        async fn set(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<()> {
            Ok(())
        }
        async fn remove(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn scope(&self) -> CacheScope {
            CacheScope::Process
        }
    }
    struct ProbeDriver(bool);
    impl CacheDriver for ProbeDriver {
        fn name(&self) -> &'static str {
            "probe"
        }
        fn build<'a>(
            &'a self,
            _: &'a CacheDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
            Box::pin(async move { Ok(Arc::new(ProbeBackend(self.0)) as Arc<dyn CacheBackend>) })
        }
    }

    #[tokio::test]
    async fn named_store_readiness_tracks_purged_replacement() -> Result<()> {
        let readiness = Arc::new(crate::core::Readiness::default());
        let mut builder = ProviderBuilder::new();
        builder
            .provider(ReadinessProvider(Arc::clone(&readiness)))
            .provider(CacheServiceProvider::new().with_driver(ProbeDriver(true)));
        let runtime = builder
            .start(Figment::from(Serialized::defaults(json!({"cache": {
                "default": "first",
                "stores": {"first": {"driver": "memory"}, "second": {"driver": "probe"}}
            }}))))
            .await?;
        let cache = runtime.container().resolve::<Cache>().await?;
        assert!(readiness.dependencies_ready().await);
        let _ = cache.store("second").await?;
        assert!(!readiness.dependencies_ready().await);
        cache.extend(ProbeDriver(false))?;
        cache.purge("second")?;
        let _ = cache.store("second").await?;
        assert!(readiness.dependencies_ready().await);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct DropCheck(Arc<CacheManager>, Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DropCheck {
        fn drop(&mut self) {
            self.1
                .store(self.0.drivers.try_lock().is_ok(), Ordering::SeqCst);
        }
    }
    impl CacheDriver for DropCheck {
        fn name(&self) -> &'static str {
            "drop-check"
        }
        fn build<'a>(
            &'a self,
            _: &'a CacheDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
            Box::pin(async { Ok(Arc::new(MemoryBackend::default()) as Arc<dyn CacheBackend>) })
        }
    }
    struct Replacement;
    impl CacheDriver for Replacement {
        fn name(&self) -> &'static str {
            "drop-check"
        }
        fn build<'a>(
            &'a self,
            _: &'a CacheDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
            Box::pin(async { Ok(Arc::new(MemoryBackend::default()) as Arc<dyn CacheBackend>) })
        }
    }
    #[tokio::test]
    async fn replacing_driver_drops_old_creator_outside_registry_lock() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheServiceProvider::new());
        let runtime = builder.start(sources("memory")).await?;
        let cache = runtime.container().resolve::<Cache>().await?;
        let manager = Arc::clone(&cache.manager);
        let unlocked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        cache.extend(DropCheck(manager, Arc::clone(&unlocked)))?;
        cache.extend(Replacement)?;
        assert!(unlocked.load(Ordering::SeqCst));
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct Custom;
    impl CacheDriver for Custom {
        fn name(&self) -> &'static str {
            "custom"
        }
        fn build<'a>(
            &'a self,
            _: &'a CacheDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
            Box::pin(async { Ok(Arc::new(MemoryBackend::default()) as Arc<dyn CacheBackend>) })
        }
    }
    struct Earlier;
    impl ServiceProvider for Earlier {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.resolve::<Cache>().await?.extend(Custom) })
        }
    }
    fn sources(driver: &str) -> Figment {
        Figment::from(Serialized::defaults(
            json!({"cache":{"default":"first","stores":{"first":{"driver":driver},"second":{"driver":"memory"}}}}),
        ))
    }
    struct TrackedBackend(Arc<AtomicUsize>);
    impl Drop for TrackedBackend {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl CacheBackend for TrackedBackend {
        async fn get(&self, _: &str) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }
        async fn set(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<()> {
            Ok(())
        }
        async fn remove(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn scope(&self) -> CacheScope {
            CacheScope::Process
        }
    }
    struct TrackedDriver(Arc<AtomicUsize>);
    impl CacheDriver for TrackedDriver {
        fn name(&self) -> &'static str {
            "tracked"
        }
        fn build<'a>(
            &'a self,
            _: &'a CacheDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
            let dropped = Arc::clone(&self.0);
            Box::pin(async move { Ok(Arc::new(TrackedBackend(dropped)) as Arc<dyn CacheBackend>) })
        }
    }
    struct FailAfter;
    impl ServiceProvider for FailAfter {
        fn boot<'a>(&'a self, _: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Err(Error::custom("test_failed", "failed")) })
        }
    }
    #[tokio::test]
    async fn failed_store_construction_releases_prior_backend() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(CacheServiceProvider::new().with_driver(TrackedDriver(Arc::clone(&dropped))))
            .provider(FailAfter);
        let source = Figment::from(Serialized::defaults(
            json!({"cache":{"default":"first","stores":{"first":{"driver":"tracked"}}}}),
        ));
        assert!(builder.start(source).await.is_err());
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    struct Counted(Arc<AtomicUsize>);
    impl CacheDriver for Counted {
        fn name(&self) -> &'static str {
            "custom"
        }
        fn build<'a>(
            &'a self,
            _: &'a CacheDriverConfig,
        ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(MemoryBackend::default()) as Arc<dyn CacheBackend>)
            })
        }
    }
    struct InvalidOverlay;
    impl ServiceProvider for InvalidOverlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay(
                    "cache",
                    json!({"stores": {"second": {"driver": "missing"}}}),
                )
            })
        }
    }
    #[tokio::test]
    async fn late_overlay_unknown_driver_fails_final_validation() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(CacheServiceProvider::new().with_driver(Custom))
            .provider(InvalidOverlay);
        let source = sources("custom");
        assert!(builder.start(source).await.is_err());
    }

    #[tokio::test]
    async fn late_extend_preserves_old_and_purge_rebuilds_with_new_driver() -> Result<()> {
        let old = Arc::new(AtomicUsize::new(0));
        let new = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheServiceProvider::new().with_driver(Counted(Arc::clone(&old))));
        let source = Figment::from(Serialized::defaults(
            json!({"cache":{"default":"first","stores":{"first":{"driver":"custom"},"second":{"driver":"custom"}}}}),
        ));
        let runtime = builder.start(source).await?;
        let cache = runtime.container().resolve::<Cache>().await?;
        let first = cache.store("first").await?;
        cache.extend(Counted(Arc::clone(&new)))?;
        let retained = cache.store("first").await?;
        assert!(Arc::ptr_eq(&first.backend, &retained.backend));
        let (left, right) = tokio::join!(cache.store("second"), cache.store("second"));
        assert!(Arc::ptr_eq(&left?.backend, &right?.backend));
        assert_eq!(new.load(Ordering::SeqCst), 1);
        cache.purge("first")?;
        assert!(!Arc::ptr_eq(
            &first.backend,
            &cache.store("first").await?.backend
        ));
        assert_eq!(new.load(Ordering::SeqCst), 2);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn provider_accepts_custom_and_earlier_registered_drivers() -> Result<()> {
        for earlier in [false, true] {
            let mut builder = ProviderBuilder::new();
            if earlier {
                builder.provider(Earlier);
                builder.provider(CacheServiceProvider::new());
            } else {
                builder.provider(CacheServiceProvider::new().with_driver(Custom));
            }
            let runtime = builder.start(sources("custom")).await?;
            let cache = runtime.container().resolve::<Cache>().await?;
            cache.set("key", &42, None).await?;
            assert_eq!(cache.get::<u32>("key").await?, Some(42));
            assert!(cache.store("second").await.is_ok());
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
                .await?;
        }
        Ok(())
    }
    #[tokio::test]
    async fn provider_rejects_conflict_bad_config_and_missing_driver() {
        let mut duplicate = ProviderBuilder::new();
        duplicate.provider(
            CacheServiceProvider::new()
                .with_driver(Custom)
                .with_driver(Custom),
        );
        assert!(duplicate.start(sources("custom")).await.is_err());
        let mut missing = ProviderBuilder::new();
        missing.provider(CacheServiceProvider::new());
        assert!(
            missing
                .start(sources("unknown-secret-sentinel"))
                .await
                .is_err()
        );
        let mut invalid = ProviderBuilder::new();
        invalid.provider(CacheServiceProvider::new());
        assert!(
            invalid
                .start(Figment::from(Serialized::defaults(
                    json!({"cache":{"default":"absent"}})
                )))
                .await
                .is_err()
        );
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
    fn validate(&self, config: &CacheDriverConfig) -> Result<()> {
        reject_unknown_options(config, &[])
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
    async fn flush(&self) -> Result<()> {
        // 保留 generation，避免正在运行的 tagged loader 误认旧代仍有效。
        self.values.lock().await.values.clear();
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
    fn validate(&self, config: &CacheDriverConfig) -> Result<()> {
        reject_unknown_options(config, &["path"])?;
        let path: Option<PathBuf> = config.option("path")?;
        if path.is_none() {
            return Err(Error::Config(crate::config::ConfigError::Invalid(
                "file cache requires path".into(),
            )));
        }
        Ok(())
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
        remove_cache_file(self.path(key)).await
    }
    async fn flush(&self) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.root)
            .await
            .map_err(|_| backend_failure())?;
        while let Some(entry) = entries.next_entry().await.map_err(|_| backend_failure())? {
            let name = entry.file_name();
            // 文件布局按 SHA-256 名称识别，跳过应用文件、子目录和正在写入的临时文件。
            if name.to_str().is_some_and(|name| {
                name.len() == 64
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            }) {
                match entry.file_type().await {
                    Ok(kind) if kind.is_file() => remove_cache_file(entry.path()).await?,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(backend_failure()),
                }
            }
        }
        Ok(())
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

async fn remove_cache_file(path: PathBuf) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(backend_failure()),
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
    fn validate(&self, config: &CacheDriverConfig) -> Result<()> {
        reject_unknown_options(config, &["dsn"])?;
        let dsn: Option<String> = config.option("dsn")?;
        let _ = redis::Client::open(dsn.unwrap_or_else(|| "redis://127.0.0.1:6379/".into()))
            .map_err(|_| {
                Error::Config(crate::config::ConfigError::Invalid(
                    "invalid Redis cache configuration".into(),
                ))
            })?;
        Ok(())
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
const REDIS_TAG_EPOCH_KEY: &str = "__cache_tag_epoch_v2";

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
    async fn flush(&self) -> Result<()> {
        redis::cmd("FLUSHDB")
            .query_async::<()>(&mut self.connection.clone())
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
        let (value, epoch): (Option<String>, Option<String>) = redis::cmd("MGET")
            .arg(tag_key)
            .arg(REDIS_TAG_EPOCH_KEY)
            .query_async(&mut connection)
            .await
            .map_err(|_| backend_failure())?;
        if let Some(epoch) = epoch {
            return value
                .unwrap_or(epoch)
                .parse()
                .map_err(|_| backend_failure());
        }
        let value: String = redis::cmd("EVAL")
            .arg(
                "
                redis.call('SET', KEYS[2], ARGV[1], 'NX')
                return redis.call('GET', KEYS[1]) or redis.call('GET', KEYS[2])",
            )
            .arg(2)
            .arg(tag_key)
            .arg(REDIS_TAG_EPOCH_KEY)
            .arg(redis_tag_seed()?)
            .query_async(&mut connection)
            .await
            .map_err(|_| backend_failure())?;
        value.parse().map_err(|_| backend_failure())
    }

    async fn invalidate(&self, tag_key: &str) -> Result<()> {
        redis_tags::invalidate(&mut self.connection.clone(), tag_key).await
    }

    async fn set_if_generations_match(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
        generations: &[(String, u64)],
    ) -> Result<bool> {
        redis_tags::write(&mut self.connection.clone(), key, value, ttl, generations).await
    }
}

#[cfg(feature = "cache-redis")]
fn redis_tag_seed() -> Result<u64> {
    // 清库后重新初始化，避免旧快照重新匹配。为 Redis 的有符号 INCR 保留空间。
    let mut seed = [0_u8; 8];
    getrandom::fill(&mut seed).map_err(|_| backend_failure())?;
    Ok((u64::from_ne_bytes(seed) >> 2) + 1)
}

#[cfg(not(feature = "cache-redis"))]
struct RedisUnavailableDriver;
#[cfg(not(feature = "cache-redis"))]
impl CacheDriver for RedisUnavailableDriver {
    fn name(&self) -> &'static str {
        "redis"
    }
    fn validate(&self, _config: &CacheDriverConfig) -> Result<()> {
        Err(Error::Config(crate::config::ConfigError::Invalid(
            "cache driver redis requires cache-redis feature".into(),
        )))
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
        static TASKS: std::sync::OnceLock<crate::foundation::lifecycle::BackgroundTasks> =
            std::sync::OnceLock::new();
        let tasks = TASKS.get_or_init(crate::foundation::lifecycle::BackgroundTasks::default);
        store_with_runtime(backend, namespace, config, tasks)
    }

    fn store_with_runtime(
        backend: Arc<dyn CacheBackend>,
        namespace: &str,
        config: &CacheSingleflightConfig,
        tasks: &crate::foundation::lifecycle::BackgroundTasks,
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
            memo: None,
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
            memo: None,
        };
        for key in [
            "__cache_tag_meta_v1:reserved",
            "__openrust_cache_tag_meta_v1:reserved",
            "__tenant_deadbeef:reserved",
            "__openrust_tenant_deadbeef:reserved",
        ] {
            assert!(unnamespaced.set(key, &1, None).await.is_err());
            assert!(cache_store.set(key, &1, None).await.is_err());
        }
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
    async fn shared_store_keys_and_tag_generations_are_tenant_scoped() -> Result<()> {
        let backend: Arc<dyn CacheBackend> = Arc::new(MemoryBackend::default());
        let cache = store(backend, "shared");
        let tenant_a = crate::tenant::TenantId::new("tenant-a")?;
        let tenant_b = crate::tenant::TenantId::new("tenant-b")?;
        crate::tenant::TenantContext::scope(tenant_a, async {
            cache.set("profile", &"A", None).await?;
            let tagged = cache.tagged(&["profiles"])?;
            tagged.set("profile", &"A", Duration::from_secs(30)).await?;
            Ok::<_, Error>(())
        })
        .await??;
        crate::tenant::TenantContext::scope(tenant_b, async {
            assert_eq!(cache.get::<String>("profile").await?, None);
            cache.set("profile", &"B", None).await?;
            let tagged = cache.tagged(&["profiles"])?;
            tagged.set("profile", &"B", Duration::from_secs(30)).await?;
            cache.invalidate_tags(&["profiles"]).await?;
            assert_eq!(tagged.get::<String>("profile").await?, None);
            Ok::<_, Error>(())
        })
        .await??;
        crate::tenant::TenantContext::scope(crate::tenant::TenantId::new("tenant-a")?, async {
            assert_eq!(cache.get::<String>("profile").await?, Some("A".into()));
            assert_eq!(
                cache
                    .tagged(&["profiles"])?
                    .get::<String>("profile")
                    .await?,
                Some("A".into())
            );
            Ok::<_, Error>(())
        })
        .await??;
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
        let tasks = crate::foundation::lifecycle::BackgroundTasks::default();
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
        let tasks = crate::foundation::lifecycle::BackgroundTasks::default();
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
        let tasks = crate::foundation::lifecycle::BackgroundTasks::default();
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

        let tasks = crate::foundation::lifecycle::BackgroundTasks::default();
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
        let tasks = crate::foundation::lifecycle::BackgroundTasks::default();
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
        use crate::foundation::ProviderBuilder;
        use figment::providers::{Format, Toml};

        let mut missing_builder = ProviderBuilder::new();
        missing_builder.provider(CacheServiceProvider::new());
        let missing = missing_builder.start(figment::Figment::new()).await;
        assert!(missing.is_err());

        let invalid_config = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault = 'primary'\n[cache.stores.primary]\ndriver = 'memory'\n[cache.singleflight]\nmax_in_flight_keys = 0\n",
        ));
        let mut invalid_builder = ProviderBuilder::new();
        invalid_builder.provider(CacheServiceProvider::new());
        assert!(invalid_builder.start(invalid_config).await.is_err());

        let invalid_swr = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault = 'primary'\n[cache.stores.primary]\ndriver = 'memory'\n[cache.swr]\nmax_in_flight_refreshes = 0\n",
        ));
        let mut invalid_swr_builder = ProviderBuilder::new();
        invalid_swr_builder.provider(CacheServiceProvider::new());
        assert!(invalid_swr_builder.start(invalid_swr).await.is_err());

        let figment = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault = 'primary'\n[cache.stores.primary]\ndriver = 'memory'\nnamespace = 'main'\n[cache.stores.secondary]\ndriver = 'memory'\nnamespace = 'other'\n",
        ));
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheServiceProvider::new());
        let app = builder.start(figment).await?;
        let cache = app.container().resolve::<Cache>().await?;
        assert_eq!(cache.default_store().await?.scope(), CacheScope::Process);
        assert_eq!(cache.store("secondary").await?.scope(), CacheScope::Process);
        assert!(cache.store("unknown").await.is_err());
        cache.set("key", &"value", None).await?;
        assert_eq!(cache.get::<String>("key").await?, Some("value".into()));
        Ok(())
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
        use crate::foundation::ProviderBuilder;
        use figment::providers::{Format, Toml};

        let figment = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault = 'primary'\n[cache.stores.primary]\ndriver = 'custom-test'\nsetting = 'parsed'\n",
        ));
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheServiceProvider::new().with_driver(CustomDriver));
        let app = builder.start(figment).await?;
        let cache = app.container().resolve::<Cache>().await?;
        cache.set("key", &42, None).await?;
        assert_eq!(cache.get::<i32>("key").await?, Some(42));
        Ok(())
    }

    #[cfg(feature = "cache-redis")]
    #[tokio::test]
    async fn redis_connection_failure_does_not_fall_back_to_memory() {
        use crate::foundation::ProviderBuilder;
        use figment::providers::{Format, Toml};

        let figment = figment::Figment::new().merge(Toml::string(
            "[cache]\ndefault = 'primary'\n[cache.stores.primary]\ndriver = 'redis'\ndsn = 'redis://127.0.0.1:1/'\n",
        ));
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheServiceProvider::new());
        assert!(builder.start(figment).await.is_err());
    }

    #[cfg(feature = "cache-redis")]
    #[tokio::test]
    async fn redis_store_roundtrips_ttl_on_live_service() -> Result<()> {
        let dsn = std::env::var("OPENRUST_TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_owned());
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(Error::internal)?;
        let namespace_id = format!("{:032x}", u128::from_le_bytes(nonce));
        let config = CacheDriverConfig {
            store_name: "integration".into(),
            namespace: format!("test:{namespace_id}"),
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
            namespace: format!("cross-client:{namespace_id}"),
            options: [("dsn".to_owned(), serde_json::Value::String(dsn))].into(),
        };
        let first_backend = RedisDriver.build(&config).await?;
        let second_backend = RedisDriver.build(&config).await?;
        let first_store = store(first_backend, &config.namespace);
        let second = store(second_backend, &config.namespace);
        let first = first_store.tagged(&["accounts"])?;
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
        let tenant_key = "tenant-profile";
        crate::tenant::TenantContext::scope(
            crate::tenant::TenantId::new("redis-tenant-a")?,
            async {
                first_store.set(tenant_key, &"A", None).await?;
                Ok::<_, Error>(())
            },
        )
        .await??;
        crate::tenant::TenantContext::scope(
            crate::tenant::TenantId::new("redis-tenant-b")?,
            async {
                assert_eq!(second.get::<String>(tenant_key).await?, None);
                second.set(tenant_key, &"B", None).await?;
                second.invalidate_tags(&["accounts"]).await?;
                Ok::<_, Error>(())
            },
        )
        .await??;
        crate::tenant::TenantContext::scope(
            crate::tenant::TenantId::new("redis-tenant-a")?,
            async {
                assert_eq!(second.get::<String>(tenant_key).await?, Some("A".into()));
                second.remove(tenant_key).await?;
                Ok::<_, Error>(())
            },
        )
        .await??;
        crate::tenant::TenantContext::scope(
            crate::tenant::TenantId::new("redis-tenant-b")?,
            async {
                second.remove(tenant_key).await?;
                Ok::<_, Error>(())
            },
        )
        .await??;
        Ok(())
    }
}
