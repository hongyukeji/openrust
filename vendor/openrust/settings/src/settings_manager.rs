use crate::database_settings_repository::{MAX_BYTES, MAX_PROPERTIES, identifier, property};
use crate::{
    SettingsError as Error, SettingsFuture, SettingsRepository, SettingsResult, SettingsSnapshot,
    SettingsUpdate,
};
use openrust::{
    cache::{Repository as Cache, Ttl, Value},
    contracts::cache::Repository as _,
    database::Transaction,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// 与部署实例数无关；命名空间必须稳定，不能使用节点名或可变显示名称。
#[derive(Clone, Debug)]
pub struct SettingsOptions {
    pub namespace: String,
    /// 自动版本检查间隔；零表示关闭后台及读取时的自动检查，保留显式刷新。
    pub refresh_interval: Duration,
    pub cache_ttl: Duration,
}
impl Default for SettingsOptions {
    fn default() -> Self {
        Self {
            namespace: "settings".into(),
            refresh_interval: Duration::from_secs(60),
            cache_ttl: Duration::from_secs(3600),
        }
    }
}
/// 通知只携带定位与版本信息，不携带配置值；版本是提示，不能替代数据库对账。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SettingsNotification {
    pub namespace: String,
    pub group: String,
    pub version: Option<String>,
}
#[derive(Clone, Debug)]
pub struct SettingsChange {
    pub namespace: String,
    pub group: String,
    pub previous_version: Option<String>,
    pub version: Option<String>,
}
/// 状态只属于当前进程。applied 为所有已注册处理器成功处理的版本。
#[derive(Clone, Debug)]
pub struct SettingsStatus {
    pub observed_version: Option<String>,
    pub applied_version: Option<String>,
    pub pending_handlers: usize,
    pub last_error: Option<Error>,
}
type Callback = Arc<dyn Fn(SettingsChange) -> SettingsFuture<'static, ()> + Send + Sync>;
struct CallbackTask(tokio::task::JoinHandle<SettingsResult<()>>);
impl Drop for CallbackTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(task) = self
            .task
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}
struct Handler {
    callback: Callback,
    applied: Option<Option<String>>,
}
struct Group {
    snapshot: Option<SettingsSnapshot>,
    checked: Option<Instant>,
    signal: Option<Option<String>>,
    handlers: BTreeMap<u64, Handler>,
    dispatch: Arc<Mutex<()>>,
    error: Option<Error>,
}
impl Default for Group {
    fn default() -> Self {
        Self {
            snapshot: None,
            checked: None,
            signal: None,
            handlers: BTreeMap::new(),
            dispatch: Arc::new(Mutex::new(())),
            error: None,
        }
    }
}
struct Inner {
    source: Arc<dyn SettingsRepository>,
    cache: Option<Arc<Cache>>,
    options: SettingsOptions,
    groups: Mutex<BTreeMap<String, Group>>,
    gate: Mutex<()>,
    started: AtomicBool,
    stopped: AtomicBool,
    next_handler: AtomicU64,
    cache_failures: AtomicU64,
    cache_retry: std::sync::Mutex<Option<Instant>>,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}
/// 版本化仓储和进程协调器。首次实际读取才启动检查，注册／启动不访问数据库。
#[derive(Clone)]
pub struct SettingsManager {
    inner: Arc<Inner>,
}
/// 给变更处理器保留弱引用，避免处理器与管理器互相持有导致无法释放。
#[derive(Clone)]
pub struct WeakSettingsManager(Weak<Inner>);
impl WeakSettingsManager {
    pub fn upgrade(&self) -> Option<SettingsManager> {
        self.0.upgrade().map(|inner| SettingsManager { inner })
    }
}
impl std::fmt::Debug for SettingsManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsManager")
            .field("namespace", &self.inner.options.namespace)
            .finish_non_exhaustive()
    }
}
impl SettingsManager {
    pub fn new(
        source: Arc<dyn SettingsRepository>,
        options: SettingsOptions,
        cache: Option<Arc<Cache>>,
    ) -> SettingsResult<Self> {
        if options.namespace.is_empty()
            || options.namespace.len() > 1024
            || options.namespace.chars().any(char::is_control)
            || (!options.refresh_interval.is_zero()
                && options.refresh_interval < Duration::from_millis(10))
            || options.refresh_interval > Duration::from_secs(86400)
            || options.cache_ttl.is_zero()
            || options.cache_ttl > Duration::from_secs(604800)
        {
            return Err(Error::InvalidDefinition);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                source,
                cache,
                options,
                groups: Mutex::new(BTreeMap::new()),
                gate: Mutex::new(()),
                started: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
                next_handler: AtomicU64::new(1),
                cache_failures: AtomicU64::new(0),
                cache_retry: std::sync::Mutex::new(None),
                task: std::sync::Mutex::new(None),
            }),
        })
    }
    pub fn downgrade(&self) -> WeakSettingsManager {
        WeakSettingsManager(Arc::downgrade(&self.inner))
    }
    pub fn namespace(&self) -> &str {
        &self.inner.options.namespace
    }
    pub fn cache_failures(&self) -> u64 {
        self.inner.cache_failures.load(Ordering::Relaxed)
    }
    /// 停止本管理器的自动检查；显式 poll/notify/load 仍可使用。不会关闭共享连接。
    pub async fn shutdown(&self) {
        self.inner.stopped.store(true, Ordering::Release);
        let task = self
            .inner
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(mut task) = task
            && tokio::time::timeout(Duration::from_secs(6), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
    fn start(&self) {
        if self.inner.options.refresh_interval.is_zero() {
            return;
        }
        // 与 shutdown 在同一把短锁下交接句柄，避免关闭后才留下新的后台任务。
        let mut slot = self
            .inner
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.inner.started.swap(true, Ordering::AcqRel)
            || self.inner.stopped.load(Ordering::Acquire)
        {
            return;
        }
        let owner: Weak<Inner> = Arc::downgrade(&self.inner);
        let interval = self
            .inner
            .options
            .refresh_interval
            .min(Duration::from_secs(1));
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(inner) = owner.upgrade() else {
                    break;
                };
                if inner.stopped.load(Ordering::Acquire) {
                    break;
                }
                let manager = Self { inner };
                let _ = manager.tick(false).await;
            }
        });
        *slot = Some(task);
    }
    fn key(&self, kind: &str, group: &str) -> String {
        let namespace: String = self
            .namespace()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("settings:{namespace}:{kind}:{group}")
    }
    fn cache_ready(&self) -> bool {
        self.inner
            .cache_retry
            .lock()
            .map(|deadline| deadline.is_none_or(|t| Instant::now() >= t))
            .unwrap_or(false)
    }
    fn cache_failed(&self) {
        self.inner.cache_failures.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut retry) = self.inner.cache_retry.lock() {
            *retry = Some(Instant::now() + Duration::from_secs(1));
        }
    }
    async fn cache_get(&self, key: &str) -> Option<serde_json::Value> {
        if !self.cache_ready() {
            return None;
        }
        let cache = self.inner.cache.as_ref()?;
        match tokio::time::timeout(Duration::from_millis(250), cache.get(key)).await {
            Ok(Ok(Some(Value::Json(value)))) => Some(value),
            Ok(Ok(None)) => None,
            _ => {
                self.cache_failed();
                None
            }
        }
    }
    async fn cache_put(&self, key: &str, value: serde_json::Value) {
        if !self.cache_ready() {
            return;
        }
        let Some(cache) = &self.inner.cache else {
            return;
        };
        let value = Value::Json(value);
        if !matches!(
            tokio::time::timeout(
                Duration::from_millis(250),
                cache.put(
                    key,
                    &value,
                    Some(Ttl::Seconds(
                        self.inner.options.cache_ttl.as_secs().max(1) as i64
                    ))
                )
            )
            .await,
            Ok(Ok(true))
        ) {
            self.cache_failed();
        }
    }
    async fn snapshots_for(
        &self,
        versions: BTreeMap<String, Option<String>>,
    ) -> SettingsResult<()> {
        for (group, version) in versions {
            let current = {
                self.inner
                    .groups
                    .lock()
                    .await
                    .get(&group)
                    .and_then(|g| g.snapshot.clone())
            };
            if current.as_ref().is_some_and(|s| s.version == version) {
                let mut groups = self.inner.groups.lock().await;
                if let Some(g) = groups.get_mut(&group) {
                    g.checked = Some(Instant::now());
                }
                continue;
            }
            let key = self.key("snapshot", &group);
            let cached = self.cache_get(&key).await.and_then(|value| {
                if serde_json::to_vec(&value).ok()?.len() > MAX_BYTES * 2 {
                    return None;
                }
                let snapshot: SettingsSnapshot = serde_json::from_value(value).ok()?;
                (snapshot.version == version
                    && snapshot.properties.len() <= MAX_PROPERTIES
                    && snapshot.properties.keys().all(|key| property(key)))
                .then_some(snapshot)
            });
            let snapshot = match cached {
                Some(snapshot) => snapshot,
                None => {
                    tokio::time::timeout(Duration::from_secs(5), self.inner.source.load(&group))
                        .await
                        .map_err(|_| Error::Database)??
                }
            };
            // 完整读取可以比版本批查询更新；以完整快照为准，绝不强行写回旧提示版本。
            self.install(&group, snapshot).await?;
        }
        Ok(())
    }
    async fn install(&self, group: &str, snapshot: SettingsSnapshot) -> SettingsResult<()> {
        if (snapshot.version.is_none() && !snapshot.properties.is_empty())
            || snapshot
                .version
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 128 || v.chars().any(char::is_control))
        {
            return Err(Error::InvalidValue);
        }
        let actual_key = self.key("snapshot", group);
        if let Ok(value) = serde_json::to_value(&snapshot) {
            self.cache_put(&actual_key, value).await;
        }
        let mut groups = self.inner.groups.lock().await;
        let entry = groups.entry(group.into()).or_default();
        if entry.snapshot.is_none() {
            for handler in entry.handlers.values_mut() {
                handler.applied = Some(snapshot.version.clone());
            }
        }
        entry.snapshot = Some(snapshot);
        entry.checked = Some(Instant::now());
        Ok(())
    }
    async fn reconcile(&self, groups: &[String], reuse_fresh: bool) -> SettingsResult<()> {
        if groups.is_empty() {
            return Ok(());
        }
        let result = async {
            let _gate = self.inner.gate.lock().await;
            // 并发读取等到锁后再核对快照，避免同一轮过期引发重复数据库检查。
            let due: Vec<String> = if reuse_fresh {
                let state = self.inner.groups.lock().await;
                groups
                    .iter()
                    .filter(|name| {
                        !state.get(*name).is_some_and(|g| {
                            g.snapshot.is_some()
                                && g.checked.is_some_and(|checked| {
                                    self.inner.options.refresh_interval.is_zero()
                                        || checked.elapsed() < self.inner.options.refresh_interval
                                })
                        })
                    })
                    .cloned()
                    .collect()
            } else {
                groups.to_vec()
            };
            if due.is_empty() {
                return Ok(());
            }
            let groups = &due;
            let versions =
                tokio::time::timeout(Duration::from_secs(5), self.inner.source.versions(groups))
                    .await
                    .map_err(|_| Error::Database)??;
            if versions.len() != groups.len() || groups.iter().any(|g| !versions.contains_key(g)) {
                return Err(Error::InvalidValue);
            }
            self.snapshots_for(versions).await
        }
        .await;
        if let Err(error) = result {
            let mut state = self.inner.groups.lock().await;
            for group in groups {
                if let Some(g) = state.get_mut(group) {
                    g.error = Some(error);
                    g.checked = None;
                }
            }
            return Err(error);
        }
        Ok(())
    }
    async fn tick(&self, force: bool) -> SettingsResult<()> {
        let active: Vec<String> = {
            self.inner
                .groups
                .lock()
                .await
                .iter()
                .filter(|(_, g)| g.snapshot.is_some())
                .map(|(key, _)| key.clone())
                .collect()
        };
        let mut due = Vec::new();
        for group in &active {
            let signal = self
                .cache_get(&self.key("changed", group))
                .await
                .and_then(|v| serde_json::from_value::<SettingsNotification>(v).ok());
            let mut groups = self.inner.groups.lock().await;
            let entry = groups.get_mut(group).ok_or(Error::InvalidDefinition)?;
            let changed = signal.as_ref().is_some_and(|s| {
                s.namespace == self.namespace()
                    && s.group == *group
                    && entry.signal.as_ref() != Some(&s.version)
            });
            if let Some(signal) =
                signal.filter(|s| s.namespace == self.namespace() && s.group == *group)
            {
                entry.signal = Some(signal.version);
            }
            if force
                || changed
                || entry
                    .checked
                    .is_none_or(|t| t.elapsed() >= self.inner.options.refresh_interval)
            {
                due.push(group.clone());
            }
        }
        let result = self.reconcile(&due, false).await;
        // 版本未变也重试失败的处理器；成功处理器按版本去重。
        for group in &active {
            self.dispatch(group).await;
        }
        result
    }
    /// 主动对账所有实际使用过的组；启动、重连或运维可使用同一入口。
    pub async fn poll(&self) -> SettingsResult<()> {
        self.tick(true).await
    }
    /// 可接任何通知适配器；重复提示不重复处理，陌生命名空间不查询数据库。
    pub async fn notify(&self, notification: &SettingsNotification) -> SettingsResult<bool> {
        if notification.namespace != self.namespace() {
            return Ok(false);
        }
        if !identifier(&notification.group) {
            return Err(Error::InvalidDefinition);
        }
        let known = {
            let state = self.inner.groups.lock().await;
            state
                .get(&notification.group)
                .and_then(|g| g.snapshot.as_ref())
                .map(|s| s.version.clone())
        };
        let Some(previous) = known else {
            return Ok(false);
        };
        if previous == notification.version {
            self.dispatch(&notification.group).await;
            return Ok(false);
        }
        self.reconcile(std::slice::from_ref(&notification.group), false)
            .await?;
        self.dispatch(&notification.group).await;
        let current = self
            .inner
            .groups
            .lock()
            .await
            .get(&notification.group)
            .and_then(|g| g.snapshot.as_ref())
            .map(|s| s.version.clone());
        Ok(current != Some(previous))
    }
    /// 处理器应幂等，用于失效／重建服务；不改变正在编辑的 Settings 实例。
    pub async fn on_change<F>(&self, group: &str, callback: F) -> SettingsResult<u64>
    where
        F: Fn(SettingsChange) -> SettingsFuture<'static, ()> + Send + Sync + 'static,
    {
        if !identifier(group) {
            return Err(Error::InvalidDefinition);
        }
        let mut groups = self.inner.groups.lock().await;
        if !groups.contains_key(group) && groups.len() >= 256 {
            return Err(Error::LimitExceeded);
        }
        let entry = groups.entry(group.into()).or_default();
        if entry.handlers.len() >= 1024 {
            return Err(Error::LimitExceeded);
        }
        let id = self.inner.next_handler.fetch_add(1, Ordering::Relaxed);
        entry.handlers.insert(
            id,
            Handler {
                callback: Arc::new(callback),
                applied: entry.snapshot.as_ref().map(|s| s.version.clone()),
            },
        );
        Ok(id)
    }
    pub async fn forget_handler(&self, group: &str, id: u64) -> bool {
        self.inner
            .groups
            .lock()
            .await
            .get_mut(group)
            .is_some_and(|g| g.handlers.remove(&id).is_some())
    }
    pub async fn status(&self, group: &str) -> Option<SettingsStatus> {
        let groups = self.inner.groups.lock().await;
        let entry = groups.get(group)?;
        let snapshot = entry.snapshot.as_ref()?;
        let pending = entry
            .handlers
            .values()
            .filter(|h| h.applied.as_ref() != Some(&snapshot.version))
            .count();
        Some(SettingsStatus {
            observed_version: snapshot.version.clone(),
            applied_version: if pending == 0 {
                snapshot.version.clone()
            } else {
                None
            },
            pending_handlers: pending,
            last_error: entry.error,
        })
    }
    async fn dispatch(&self, group: &str) {
        let gate = {
            self.inner
                .groups
                .lock()
                .await
                .get(group)
                .map(|g| g.dispatch.clone())
        };
        let Some(gate) = gate else {
            return;
        };
        let Ok(_serial) = gate.try_lock() else {
            return;
        };
        for _ in 0..8 {
            let pending = {
                let groups = self.inner.groups.lock().await;
                let Some(entry) = groups.get(group) else {
                    return;
                };
                let Some(snapshot) = &entry.snapshot else {
                    return;
                };
                entry
                    .handlers
                    .iter()
                    .filter(|(_, h)| h.applied.as_ref() != Some(&snapshot.version))
                    .map(|(id, h)| {
                        (
                            *id,
                            h.callback.clone(),
                            SettingsChange {
                                namespace: self.namespace().into(),
                                group: group.into(),
                                previous_version: h.applied.clone().flatten(),
                                version: snapshot.version.clone(),
                            },
                        )
                    })
                    .collect::<Vec<_>>()
            };
            if pending.is_empty() {
                let mut groups = self.inner.groups.lock().await;
                if let Some(g) = groups.get_mut(group)
                    && g.checked.is_some()
                {
                    g.error = None;
                }
                return;
            }
            let mut failed = false;
            for (id, callback, event) in pending {
                let version = event.version.clone();
                let future =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event)));
                let result = match future {
                    Err(_) => Err(Error::Refresh),
                    Ok(future) => {
                        let mut task = CallbackTask(tokio::spawn(future));
                        match tokio::time::timeout(Duration::from_secs(5), &mut task.0).await {
                            Ok(Ok(result)) => result,
                            Ok(Err(_)) => Err(Error::Refresh),
                            Err(_) => {
                                task.0.abort();
                                let _ = (&mut task.0).await;
                                Err(Error::Refresh)
                            }
                        }
                    }
                };
                let mut groups = self.inner.groups.lock().await;
                let Some(g) = groups.get_mut(group) else {
                    continue;
                };
                match result {
                    Ok(()) => {
                        if let Some(h) = g.handlers.get_mut(&id) {
                            h.applied = Some(version);
                        }
                    }
                    Err(error) => {
                        g.error = Some(error);
                        failed = true;
                    }
                }
            }
            if failed {
                return;
            }
        }
    }
    async fn announce(&self, group: &str, version: Option<String>) {
        let signal = SettingsNotification {
            namespace: self.namespace().into(),
            group: group.into(),
            version,
        };
        if let Ok(value) = serde_json::to_value(signal) {
            self.cache_put(&self.key("changed", group), value).await;
        }
        if let Some(g) = self.inner.groups.lock().await.get_mut(group) {
            g.checked = None;
        }
    }
    async fn published(&self, group: &str, version: Option<String>) {
        self.announce(group, version).await;
        // 数据库提交已成功；缓存、通知或处理器失败不能伪装成事务回滚。
        let _ = self.reconcile(&[group.into()], false).await;
    }
}
impl SettingsRepository for SettingsManager {
    fn load<'a>(&'a self, group: &'a str) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            if !identifier(group) {
                return Err(Error::InvalidDefinition);
            }
            self.start();
            {
                let mut groups = self.inner.groups.lock().await;
                if !groups.contains_key(group) && groups.len() >= 256 {
                    return Err(Error::LimitExceeded);
                }
                groups.entry(group.into()).or_default();
            }
            let cached = {
                let groups = self.inner.groups.lock().await;
                let entry = &groups[group];
                entry
                    .checked
                    .filter(|t| {
                        self.inner.options.refresh_interval.is_zero()
                            || t.elapsed() < self.inner.options.refresh_interval
                    })
                    .and_then(|_| entry.snapshot.clone())
            };
            if let Some(snapshot) = cached {
                return Ok(snapshot);
            }
            self.reconcile(&[group.into()], true).await?;
            self.inner
                .groups
                .lock()
                .await
                .get(group)
                .and_then(|g| g.snapshot.clone())
                .ok_or(Error::Database)
        })
    }
    fn refresh<'a>(&'a self, group: &'a str) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            if !identifier(group) {
                return Err(Error::InvalidDefinition);
            }
            {
                let mut groups = self.inner.groups.lock().await;
                if !groups.contains_key(group) && groups.len() >= 256 {
                    return Err(Error::LimitExceeded);
                }
                groups.entry(group.into()).or_default();
            }
            let result = async {
                let _gate = self.inner.gate.lock().await;
                let snapshot =
                    tokio::time::timeout(Duration::from_secs(5), self.inner.source.refresh(group))
                        .await
                        .map_err(|_| Error::Database)??;
                self.install(group, snapshot.clone()).await?;
                Ok(snapshot)
            }
            .await;
            match result {
                Ok(snapshot) => {
                    self.start();
                    Ok(snapshot)
                }
                Err(error) => {
                    let mut groups = self.inner.groups.lock().await;
                    if let Some(g) = groups.get_mut(group) {
                        g.error = Some(error);
                        g.checked = None;
                    }
                    Err(error)
                }
            }
        })
    }
    fn versions<'a>(
        &'a self,
        groups: &'a [String],
    ) -> SettingsFuture<'a, BTreeMap<String, Option<String>>> {
        self.inner.source.versions(groups)
    }
    fn save<'a>(
        &'a self,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            let snapshot = self.inner.source.save(group, expected, changes).await?;
            self.published(group, snapshot.version.clone()).await;
            Ok(snapshot)
        })
    }
    fn save_in<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot> {
        Box::pin(async move {
            let snapshot = self
                .inner
                .source
                .save_in(transaction, group, expected, changes)
                .await?;
            let manager = self.clone();
            let group = group.to_owned();
            let version = snapshot.version.clone();
            transaction
                .after_commit(move || {
                    Box::pin(async move {
                        manager.announce(&group, version).await;
                        Ok(())
                    })
                })
                .map_err(|_| Error::Commit)?;
            Ok(snapshot)
        })
    }
}
