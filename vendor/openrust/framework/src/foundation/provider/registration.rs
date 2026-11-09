//! 启动与已启动 App 共用的 Provider 注册执行器；嵌套调用不复制生命周期。

use std::any::TypeId;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use super::deferred::{DeferredProviders, Definition};
use super::{
    BootContext, BootHook, ConfigRepository, Container, Extensions, Finalizer,
    ProviderBootCallbacks, ProviderId, RegisterContext, Required, ResourceRegistrar,
    StartupCallbacks, check_dependencies,
};
use crate::console::command_registry::CommandRegistry;
use crate::core::{Error, Result};
use crate::support::BoxFuture;
use crate::support::ServiceProvider;

/// 有效实例及启动移除规则；用户回调不在正在注册身份的锁内执行。
pub(crate) struct ProviderRegistry {
    providers: Mutex<Vec<Arc<RegisteredProvider>>>,
    registering: Arc<Mutex<HashSet<TypeId>>>,
    removed: HashSet<ProviderId>,
    loader: OnceLock<Weak<DeferredProviders>>,
    pub(crate) startup_callbacks: StartupCallbacks,
}

/// 实际注册的 Provider 实例。延迟装载监听器可返回此对象作为事件响应。
pub struct RegisteredProvider {
    id: ProviderId,
    provider: Option<Box<dyn ServiceProvider>>,
    callbacks: tokio::sync::Mutex<ProviderBootCallbacks>,
}

impl RegisteredProvider {
    /// 获取本实例的具体 Provider 类型身份。
    pub const fn id(&self) -> ProviderId {
        self.id
    }

    /// 借用注册表中的原 Provider 实例。
    pub fn provider(&self) -> Option<&dyn ServiceProvider> {
        self.provider.as_deref()
    }

    pub(super) async fn boot(&self, context: &mut BootContext<'_>) -> Result<()> {
        let provider = self
            .provider
            .as_deref()
            .ok_or_else(|| Error::custom("provider_retired", "Provider 实例已回收"))?;
        let mut callbacks = self.callbacks.lock().await;
        context.boot_provider(provider, &mut callbacks).await
    }

    pub(crate) fn dispose(self: Arc<Self>) -> bool {
        Arc::into_inner(self).is_some_and(|mut owned| owned.dispose_inner())
    }

    fn dispose_inner(&mut self) -> bool {
        use crate::foundation::lifecycle::disposal;
        let callbacks_failed = self.callbacks.get_mut().dispose();
        let provider_failed = disposal::dispose(self.provider.take(), None).seen;
        callbacks_failed || provider_failed
    }
}

impl crate::support::DataObject for RegisteredProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Drop for RegisteredProvider {
    fn drop(&mut self) {
        // 快照可能延后最终析构。回收回调和 Provider 都在注册表锁外进行。
        if self.dispose_inner() {
            tracing::error!(provider = self.id.name(), "Provider disposal failed");
        }
    }
}

impl ProviderRegistry {
    pub(crate) fn new(removed: HashSet<ProviderId>) -> Self {
        Self {
            providers: Mutex::default(),
            registering: Arc::default(),
            removed,
            loader: OnceLock::new(),
            startup_callbacks: StartupCallbacks::default(),
        }
    }

    pub(super) fn declare_deferred(
        &self,
        container: &Container,
        definition: Definition,
    ) -> Result<()> {
        if !self.startup_callbacks.is_booted() && self.removed.contains(&definition.id) {
            return Ok(());
        }
        let loader = self.loader.get().and_then(Weak::upgrade).ok_or_else(|| {
            Error::custom(
                "deferred_provider_retired",
                "延迟 Provider 的应用注册表已回收",
            )
        })?;
        loader.add(container, definition)
    }

    pub(super) fn attach_loader(&self, loader: &Arc<DeferredProviders>) {
        let _ = self.loader.set(Arc::downgrade(loader));
    }

    pub(super) fn event_hooks(&self) -> crate::events::DeferredEventHooks {
        self.loader
            .get()
            .and_then(Weak::upgrade)
            .map_or_else(crate::events::DeferredEventHooks::default, |loader| {
                loader.event_hooks()
            })
    }

    fn providers(&self) -> std::sync::MutexGuard<'_, Vec<Arc<RegisteredProvider>>> {
        self.providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(super) fn boot_snapshot(&self) -> Vec<Arc<RegisteredProvider>> {
        self.providers().clone()
    }

    pub(super) fn next_startup_provider(&self, index: usize) -> Option<Arc<RegisteredProvider>> {
        let providers = self.providers();
        let next = providers.get(index).cloned();
        if next.is_none() {
            self.startup_callbacks.mark_booted();
        }
        next
    }

    pub(super) fn registered(&self, id: ProviderId) -> Option<Arc<RegisteredProvider>> {
        self.providers()
            .iter()
            .find(|active| active.id == id)
            .cloned()
    }

    pub(crate) fn take_active(&self) -> Vec<Arc<RegisteredProvider>> {
        std::mem::take(&mut *self.providers())
    }

    fn verify_nested(
        &self,
        parent: ProviderId,
        children: &[ProviderId],
        startup: bool,
    ) -> Result<()> {
        let providers = self.providers();
        for child in children {
            if startup && self.removed.contains(child) {
                continue;
            }
            if !providers.iter().any(|active| active.id == *child) {
                return Err(Error::custom(
                    "provider_nested_not_registered",
                    format!(
                        "{} 声明的子 {} 尚未完成 register",
                        parent.name(),
                        child.name()
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// 已启动 App 的即时 boot 上下文；启动阶段仍由原统一屏障负责 boot。
#[derive(Clone)]
pub(crate) struct LiveBoot {
    pub(crate) resources: ResourceRegistrar,
    pub(crate) extensions: Extensions,
    pub(crate) commands: CommandRegistry,
}

#[derive(Clone)]
pub(crate) enum BootMode<'a> {
    Startup,
    Live(LiveBoot),
    Deferred {
        loader: &'a DeferredProviders,
        trigger: DeferredRegistration,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum DeferredRegistration {
    Service,
    Event,
    Nested,
}

impl BootMode<'_> {
    pub(super) fn live(&self) -> Option<&LiveBoot> {
        match self {
            Self::Startup => None,
            Self::Live(live) => Some(live),
            Self::Deferred { loader, .. } => Some(&loader.live),
        }
    }

    pub(super) fn nested(&self) -> Self {
        match self {
            Self::Deferred { loader, .. } => Self::Deferred {
                loader,
                trigger: DeferredRegistration::Nested,
            },
            mode => mode.clone(),
        }
    }

    fn is_live(&self, callbacks: &StartupCallbacks) -> bool {
        match self {
            Self::Startup => false,
            Self::Live(_) => true,
            Self::Deferred { .. } => callbacks.is_booted(),
        }
    }

    fn contribute_startup(
        &self,
        registry: &ProviderRegistry,
        entry: &Arc<RegisteredProvider>,
        required: Vec<Required>,
        parent_required: &mut Vec<Required>,
    ) {
        let Self::Deferred { loader, trigger } = self else {
            parent_required.extend(required);
            return;
        };
        match trigger {
            DeferredRegistration::Event => loader.add_startup_required(required),
            DeferredRegistration::Nested => parent_required.extend(required),
            DeferredRegistration::Service => {
                let entry = Arc::clone(entry);
                registry
                    .startup_callbacks
                    .add_booting(Arc::new(move |context| {
                        let entry = Arc::clone(&entry);
                        Box::pin(async move {
                            let provider = entry.provider.as_deref().ok_or_else(|| {
                                Error::custom("provider_retired", "Provider 实例已回收")
                            })?;
                            provider.boot(context).await
                        })
                    }));
                loader.add_startup_required(required);
            }
        }
    }
}

impl LiveBoot {
    pub(crate) async fn booted(
        &self,
        container: &Container,
        config: &mut ConfigRepository,
        startup_callbacks: &StartupCallbacks,
        callback: Arc<BootHook>,
    ) -> Result<()> {
        if container.is_terminated() {
            return Err(Error::custom("container_closed", "容器已关闭"));
        }
        let mut required = Vec::new();
        let mut finalizers = Vec::new();
        BootContext {
            console_command: None,
            container,
            config,
            resources: self.resources.clone(),
            required: &mut required,
            extensions: &self.extensions,
            commands: &self.commands,
            finalizers: &mut finalizers,
            startup_callbacks,
            provider_callbacks: None,
        }
        .add_booted(callback)
        .await?;
        finish_boot(container, finalizers, required).await
    }
}

async fn finish_boot(
    container: &Container,
    finalizers: Vec<Finalizer>,
    required: Vec<Required>,
) -> Result<()> {
    for finalize in finalizers {
        finalize().await?;
    }
    for resolve in required {
        resolve(container).await?;
    }
    Ok(())
}

struct RegistrationGuard {
    registering: Arc<Mutex<HashSet<TypeId>>>,
    id: TypeId,
}

impl RegistrationGuard {
    fn enter(registry: &ProviderRegistry, id: ProviderId) -> Result<Self> {
        let inserted = registry
            .registering
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.type_id);
        if !inserted {
            return Err(Error::custom(
                "provider_registration_cycle",
                format!("{} 正在 register，不能递归重新登记", id.name()),
            ));
        }
        Ok(Self {
            registering: Arc::clone(&registry.registering),
            id: id.type_id,
        })
    }
}

impl Drop for RegistrationGuard {
    fn drop(&mut self) {
        self.registering
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.id);
    }
}

/// 保持同类型去重、force 首槽与失败贡献；`BoxFuture` 允许真正的递归 await。
pub(crate) fn register<'a>(
    registry: &'a ProviderRegistry,
    provider: Box<dyn ServiceProvider>,
    force: bool,
    container: &'a Container,
    config: &'a mut ConfigRepository,
    deferred_required: &'a mut Vec<Required>,
    mode: BootMode<'a>,
) -> BoxFuture<'a, Result<ProviderId>> {
    Box::pin(async move {
        let id = provider.id();
        if container.is_terminated() {
            return Err(Error::custom("container_closed", "容器已关闭"));
        }
        // 测试清单的移除约束覆盖同一次启动中的嵌套 force；运行期显式注册沿原 App API。
        let starting = !mode.is_live(&registry.startup_callbacks);
        if starting && registry.removed.contains(&id) {
            return Ok(id);
        }
        let (guard, ready) = {
            let providers = registry.providers();
            if !force && providers.iter().any(|active| active.id == id) {
                return Ok(id);
            }
            let guard = RegistrationGuard::enter(registry, id)?;
            let ready = providers.iter().map(|active| active.id.type_id).collect();
            (guard, ready)
        };
        check_dependencies(provider.as_ref(), &ready)?;
        let nested = provider.nested_providers();
        let mut required = Vec::new();
        let mut callbacks = ProviderBootCallbacks::default();
        provider
            .register(&mut RegisterContext {
                container,
                config,
                required: &mut required,
                owner: id,
                force,
                registry,
                boot_mode: mode.clone(),
                provider_callbacks: &mut callbacks,
            })
            .await?;
        registry.verify_nested(id, &nested, starting)?;
        let entry = Arc::new(RegisteredProvider {
            id,
            provider: Some(provider),
            callbacks: tokio::sync::Mutex::new(callbacks),
        });
        let (old, boot_now) = {
            let mut providers = registry.providers();
            // 启动遍历的最后一次读取与注册发布使用同一把锁。
            let boot_now = mode.is_live(&registry.startup_callbacks);
            if !boot_now {
                mode.contribute_startup(
                    registry,
                    &entry,
                    std::mem::take(&mut required),
                    deferred_required,
                );
            }
            let old = if let Some(index) = providers.iter().position(|active| active.id == id) {
                Some(std::mem::replace(&mut providers[index], Arc::clone(&entry)))
            } else {
                providers.push(Arc::clone(&entry));
                None
            };
            (old, boot_now)
        };
        drop(guard);
        if old.is_some_and(RegisteredProvider::dispose) {
            return Err(Error::custom(
                "resource_task_interrupted",
                "Provider 已替换，但原实例或启动回调析构异常",
            ));
        }
        if boot_now && let Some(live) = mode.live() {
            let mut finalizers: Vec<Finalizer> = Vec::new();
            entry
                .boot(&mut BootContext {
                    console_command: None,
                    container,
                    config,
                    resources: live.resources.clone(),
                    required: &mut required,
                    extensions: &live.extensions,
                    commands: &live.commands,
                    finalizers: &mut finalizers,
                    startup_callbacks: &registry.startup_callbacks,
                    provider_callbacks: None,
                })
                .await?;
            finish_boot(container, finalizers, required).await?;
        }
        Ok(id)
    })
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;
