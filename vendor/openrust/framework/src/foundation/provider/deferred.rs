//! 按服务键或事件在原 Provider 注册表中装载编译工厂。

use super::registration::{
    self, BootMode, DeferredRegistration, LiveBoot, ProviderRegistry, RegisteredProvider,
};
use super::{ProviderId, Required};
use crate::config::repository::ConfigRepository;
use crate::container::{Container, ServiceKey};
use crate::events::{DeferredEventHook, DeferredEventHooks, DeferredEventListeners};
use crate::support::{BoxFuture, DataValue, ServiceProvider};
use crate::{Error, Result};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use tokio::sync::watch;

/// 声明由首次服务解析或指定事件触发的延迟 Provider。
///
/// Rust 在应用清单中通过 `ProviderBuilder::deferred_provider` 显式登记编译工厂。
pub trait DeferrableProvider: ServiceProvider {
    /// 可触发当前 Provider 的精确具名键或 Rust 类型键。
    fn provides() -> Vec<ServiceKey>;

    /// 触发普通注册的事件名或通配模式；注册不会消耗服务键。
    fn when() -> Vec<&'static str> {
        Vec::new()
    }
}

pub(super) struct Definition {
    pub(super) id: ProviderId,
    pub(super) keys: Vec<ServiceKey>,
    pub(super) events: Vec<&'static str>,
    factory: Box<dyn Fn() -> Box<dyn ServiceProvider> + Send + Sync>,
}

impl Definition {
    pub(super) fn new<P, F>(factory: F) -> Self
    where
        P: DeferrableProvider,
        F: Fn() -> P + Send + Sync + 'static,
    {
        Self {
            id: ProviderId::of::<P>(),
            keys: P::provides(),
            events: P::when(),
            factory: Box::new(move || Box::new(factory())),
        }
    }

    pub(super) fn has_events(&self) -> bool {
        !self.events.is_empty()
    }
}

type Registered = Option<Arc<RegisteredProvider>>;
type Outcome = std::result::Result<Registered, Arc<Error>>;

#[derive(Clone, Copy)]
enum Trigger<'a> {
    Service {
        key: &'a ServiceKey,
        has_instance: bool,
    },
    Event(&'a Arc<Definition>),
}

enum Admission<'a> {
    Resolved(Registered),
    InFlight {
        attempt: Arc<Attempt>,
        definition: Option<Arc<Definition>>,
        edge: Option<WaitEdge<'a>>,
    },
}

struct Attempt {
    id: ProviderId,
    key: Option<ServiceKey>,
    outcome: watch::Sender<Option<Outcome>>,
}

impl Attempt {
    async fn wait(&self) -> Result<Registered> {
        let mut receiver = self.outcome.subscribe();
        let result = receiver
            .wait_for(Option::is_some)
            .await
            .map_err(Error::internal)?;
        match result.as_ref() {
            Some(Ok(provider)) => Ok(provider.clone()),
            Some(Err(error)) => Err(shared_error(error)),
            None => Err(Error::custom(
                "deferred_provider_interrupted",
                "延迟 Provider 装载已中断",
            )),
        }
    }
}

fn shared_error(error: &Arc<Error>) -> Error {
    Error::http_with_source(
        error.status(),
        error.code(),
        "延迟 Provider 装载失败",
        Arc::clone(error),
    )
}

tokio::task_local! {
    static CURRENT: (Weak<DeferredProviders>, Arc<Attempt>);
}

#[derive(Default)]
struct Directory {
    keys: IndexMap<ServiceKey, Arc<Definition>>,
    loading: HashMap<ProviderId, Arc<Attempt>>,
    waits: HashMap<(ProviderId, ProviderId), usize>,
    flushing: bool,
    published: bool,
}

/// 容器同步 bound 查询只读取键，不持有 Provider 工厂或装载任务。
pub(crate) struct DeferredKeys(Mutex<HashSet<ServiceKey>>);

impl DeferredKeys {
    fn keys(&self) -> MutexGuard<'_, HashSet<ServiceKey>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn contains(&self, key: &ServiceKey) -> bool {
        self.keys().contains(key)
    }
}

impl Directory {
    fn loading_for_key(&self, key: &ServiceKey) -> Option<&Arc<Attempt>> {
        self.keys.get(key).map_or_else(
            || {
                self.loading
                    .values()
                    .find(|attempt| attempt.key.as_ref() == Some(key))
            },
            |entry| self.loading.get(&entry.id),
        )
    }

    fn reaches(&self, from: ProviderId, target: ProviderId) -> bool {
        let mut pending = vec![from];
        let mut seen = HashSet::new();
        while let Some(id) = pending.pop() {
            if id == target {
                return true;
            }
            if seen.insert(id) {
                pending.extend(
                    self.waits
                        .keys()
                        .filter(|(start, _)| *start == id)
                        .map(|(_, end)| *end),
                );
            }
        }
        false
    }
}

pub(crate) struct DeferredProviders {
    directory: Mutex<Directory>,
    pub(crate) visible: Arc<DeferredKeys>,
    events: Mutex<Arc<DeferredEventListeners>>,
    registry: Weak<ProviderRegistry>,
    config: ConfigRepository,
    pub(super) live: LiveBoot,
    startup_required: Mutex<Vec<Required>>,
}

impl DeferredProviders {
    pub(super) fn new(
        registry: &Arc<ProviderRegistry>,
        config: ConfigRepository,
        live: LiveBoot,
    ) -> Arc<Self> {
        Arc::new(Self {
            visible: Arc::new(DeferredKeys(Mutex::default())),
            events: Mutex::default(),
            directory: Mutex::default(),
            registry: Arc::downgrade(registry),
            config,
            live,
            startup_required: Mutex::default(),
        })
    }

    fn events(&self) -> MutexGuard<'_, Arc<DeferredEventListeners>> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn event_hooks(&self) -> DeferredEventHooks {
        self.events().hooks()
    }

    fn retire_events(&self) -> bool {
        // 原派发器及其已取得的 clone 保留实际监听器；新派发器读取新的空目录。
        let previous = std::mem::take(&mut *self.events());
        crate::foundation::lifecycle::disposal::dispose(previous, None).seen
    }

    pub(super) fn publish_keys(self: &Arc<Self>, container: &Container) -> Result<()> {
        let has_keys = {
            let mut directory = self.directory();
            directory.published = true;
            !directory.keys.is_empty()
        };
        if has_keys {
            container.install_deferred_providers(self)?;
        }
        Ok(())
    }

    pub(super) fn add(
        self: &Arc<Self>,
        container: &Container,
        definition: Definition,
    ) -> Result<()> {
        let definition = Arc::new(definition);
        let mut displaced = Vec::new();
        let weak_loader = Arc::downgrade(self);
        let factory = container.scope_factory();
        let event_definition = Arc::clone(&definition);
        let hook: DeferredEventHook = Arc::new(move || -> BoxFuture<'static, Result<DataValue>> {
            let loader = weak_loader.upgrade();
            let container = factory.container();
            let definition = Arc::clone(&event_definition);
            Box::pin(async move {
                let (Some(loader), Some(container)) = (loader, container) else {
                    return Ok(DataValue::Null);
                };
                Ok(loader
                    .load_trigger(&container, Trigger::Event(&definition))
                    .await?
                    .map_or(DataValue::Null, |provider| DataValue::Object(provider)))
            })
        });
        let (previous_listeners, install) = {
            let mut directory = self.directory();
            if directory.flushing
                || directory.loading.contains_key(&definition.id)
                || definition
                    .keys
                    .iter()
                    .any(|key| directory.loading_for_key(key).is_some())
            {
                return Err(Error::custom(
                    "deferred_provider_in_progress",
                    "相关延迟 Provider 尚在装载或清空",
                ));
            }
            let mut visible = self.visible.keys();
            for key in &definition.keys {
                if let Some(previous) = directory.keys.insert(key.clone(), Arc::clone(&definition))
                {
                    displaced.push(previous);
                }
                visible.insert(key.clone());
            }
            (
                self.events()
                    .replace(definition.id, &definition.events, &hook),
                directory.published && !directory.keys.is_empty(),
            )
        };
        let mut failed = false;
        for previous in displaced {
            failed |= crate::foundation::lifecycle::disposal::dispose(previous, None).seen;
        }
        failed |= previous_listeners.dispose();
        if install {
            container.install_deferred_providers(self)?;
        }
        if failed {
            return Err(Error::custom(
                "resource_task_interrupted",
                "延迟 Provider 清单已更新，但旧工厂析构异常",
            ));
        }
        Ok(())
    }

    fn directory(&self) -> MutexGuard<'_, Directory> {
        self.directory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn contains(&self, key: &ServiceKey) -> bool {
        self.visible.contains(key)
    }

    pub(crate) fn handles(&self, key: &ServiceKey) -> bool {
        let directory = self.directory();
        directory.keys.contains_key(key) || directory.loading_for_key(key).is_some()
    }

    pub(super) fn add_startup_required(&self, required: Vec<Required>) {
        self.startup_required
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(required);
    }

    pub(super) fn take_startup_required(&self) -> Vec<Required> {
        std::mem::take(
            &mut *self
                .startup_required
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    pub(crate) async fn load(
        self: &Arc<Self>,
        container: &Container,
        key: &ServiceKey,
        has_instance: bool,
    ) -> Result<()> {
        self.load_trigger(container, Trigger::Service { key, has_instance })
            .await
            .map(|_| ())
    }

    async fn load_trigger(
        self: &Arc<Self>,
        container: &Container,
        trigger: Trigger<'_>,
    ) -> Result<Registered> {
        let registry = self.registry.upgrade().ok_or_else(|| {
            Error::custom(
                "deferred_provider_retired",
                "延迟 Provider 的应用注册表已回收",
            )
        })?;
        let (attempt, definition, _edge) = match self.admit(&registry, trigger)? {
            Admission::Resolved(provider) => return Ok(provider),
            Admission::InFlight {
                attempt,
                definition,
                edge,
            } => (attempt, definition, edge),
        };
        let Some(definition) = definition else {
            return attempt.wait().await;
        };
        let mut operation = LoadGuard {
            loader: self,
            attempt: Arc::clone(&attempt),
            done: false,
        };
        let registration = if attempt.key.is_some() {
            DeferredRegistration::Service
        } else {
            DeferredRegistration::Event
        };
        let outcome = CURRENT
            .scope(
                (Arc::downgrade(self), attempt),
                self.register(container, &registry, &definition, registration),
            )
            .await
            .map_err(Arc::new);
        operation.finish(outcome.clone());
        outcome.map_err(|error| shared_error(&error))
    }

    fn admit<'a>(
        self: &'a Arc<Self>,
        registry: &ProviderRegistry,
        trigger: Trigger<'_>,
    ) -> Result<Admission<'a>> {
        let parent = CURRENT
            .try_with(|(owner, attempt)| {
                (owner.as_ptr() == Arc::as_ptr(self)).then_some(attempt.id)
            })
            .ok()
            .flatten();
        let mut directory = self.directory();
        let (entry, inflight, key, has_instance) = match trigger {
            Trigger::Service { key, has_instance } => (
                directory.keys.get(key).cloned(),
                directory.loading_for_key(key).cloned(),
                Some(key),
                has_instance,
            ),
            Trigger::Event(definition) => (
                Some(Arc::clone(definition)),
                directory.loading.get(&definition.id).cloned(),
                None,
                false,
            ),
        };
        let Some(id) = inflight
            .as_ref()
            .map(|attempt| attempt.id)
            .or_else(|| entry.as_ref().map(|entry| entry.id))
        else {
            return Ok(Admission::Resolved(None));
        };
        if directory.flushing {
            return Err(Error::custom(
                "deferred_provider_flush_in_progress",
                "应用正在清空 Provider 注册",
            ));
        }
        let registered = registry.registered(id);
        if parent == Some(id) && (has_instance || entry.is_none() || registered.is_some()) {
            return Ok(Admission::Resolved(registered));
        }
        if inflight.is_none() && (has_instance || registered.is_some()) {
            return Ok(Admission::Resolved(registered));
        }
        let edge = if let Some(parent) = parent {
            if directory.reaches(id, parent) {
                if registered.is_some() {
                    return Ok(Admission::Resolved(registered));
                }
                return Err(Error::custom(
                    "deferred_provider_cycle",
                    format!("延迟 Provider 装载循环：{} → {}", parent.name(), id.name()),
                ));
            }
            let key = (parent, id);
            *directory.waits.entry(key).or_default() += 1;
            Some(WaitEdge(self, key))
        } else {
            None
        };
        if let Some(attempt) = inflight {
            Ok(Admission::InFlight {
                attempt,
                definition: None,
                edge,
            })
        } else {
            let attempt = Arc::new(Attempt {
                id,
                key: key.cloned(),
                outcome: watch::channel(None).0,
            });
            if let Some(key) = key {
                directory.keys.shift_remove(key);
                self.visible.keys().remove(key);
            }
            directory.loading.insert(id, Arc::clone(&attempt));
            Ok(Admission::InFlight {
                attempt,
                definition: entry,
                edge,
            })
        }
    }

    async fn register(
        &self,
        container: &Container,
        registry: &ProviderRegistry,
        definition: &Definition,
        trigger: DeferredRegistration,
    ) -> Result<Registered> {
        let provider = (definition.factory)();
        if provider.id() != definition.id {
            return Err(Error::custom(
                "deferred_provider_identity_mismatch",
                "延迟工厂返回了不同的 Provider 身份",
            ));
        }
        let mut config = self.config.clone();
        let mut required = Vec::new();
        registration::register(
            registry,
            provider,
            false,
            container,
            &mut config,
            &mut required,
            BootMode::Deferred {
                loader: self,
                trigger,
            },
        )
        .await?;
        Ok(registry.registered(definition.id))
    }

    pub(crate) async fn load_all(self: &Arc<Self>, container: &Container) -> Result<()> {
        if container.is_terminated() {
            return Err(Error::custom("container_closed", "容器已关闭"));
        }
        let keys: Vec<_> = {
            let directory = self.directory();
            directory
                .keys
                .keys()
                .chain(
                    directory
                        .loading
                        .values()
                        .filter_map(|attempt| attempt.key.as_ref()),
                )
                .cloned()
                .collect()
        };
        for key in keys {
            self.load(container, &key, false).await?;
        }
        if self.clear_keys() {
            return Err(Error::custom(
                "resource_task_interrupted",
                "延迟 Provider 清单已清空，但旧工厂析构异常",
            ));
        }
        Ok(())
    }

    fn clear_keys(&self) -> bool {
        use crate::foundation::lifecycle::disposal;
        let previous = {
            let mut directory = self.directory();
            self.visible.keys().clear();
            std::mem::take(&mut directory.keys)
        };
        let mut failed = false;
        for definition in previous.into_values() {
            failed |= disposal::dispose(definition, None).seen;
        }
        failed
    }

    pub(crate) fn prepare_flush(&self) -> Result<FlushGuard<'_>> {
        let mut directory = self.directory();
        if !directory.loading.is_empty() || directory.flushing {
            return Err(Error::custom(
                "deferred_provider_in_progress",
                "延迟 Provider 尚在装载",
            ));
        }
        directory.flushing = true;
        Ok(FlushGuard(self))
    }
}

struct WaitEdge<'a>(&'a DeferredProviders, (ProviderId, ProviderId));

impl Drop for WaitEdge<'_> {
    fn drop(&mut self) {
        let mut directory = self.0.directory();
        if let Some(count) = directory.waits.get_mut(&self.1) {
            *count -= 1;
            if *count == 0 {
                directory.waits.remove(&self.1);
            }
        }
    }
}

struct LoadGuard<'a> {
    loader: &'a DeferredProviders,
    attempt: Arc<Attempt>,
    done: bool,
}

impl LoadGuard<'_> {
    fn finish(&mut self, outcome: Outcome) {
        self.loader.directory().loading.remove(&self.attempt.id);
        self.done = true;
        self.attempt.outcome.send_replace(Some(outcome));
    }
}

impl Drop for LoadGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.finish(Err(Arc::new(Error::custom(
                "deferred_provider_interrupted",
                "延迟 Provider 装载已中断",
            ))));
        }
    }
}

pub(crate) struct FlushGuard<'a>(&'a DeferredProviders);

impl FlushGuard<'_> {
    pub(crate) fn clear(&self) -> bool {
        self.0.clear_keys() | self.0.retire_events()
    }
}

impl Drop for FlushGuard<'_> {
    fn drop(&mut self) {
        self.0.directory().flushing = false;
    }
}
