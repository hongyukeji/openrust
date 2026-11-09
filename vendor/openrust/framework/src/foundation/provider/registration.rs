//! 启动与已启动 App 共用的 Provider 注册执行器；嵌套调用不复制生命周期。

use std::any::TypeId;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

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
    providers: Mutex<Vec<Arc<ActiveProvider>>>,
    registering: Arc<Mutex<HashSet<TypeId>>>,
    removed: HashSet<ProviderId>,
    pub(crate) startup_callbacks: StartupCallbacks,
}

pub(crate) struct ActiveProvider {
    id: ProviderId,
    provider: Option<Box<dyn ServiceProvider>>,
    callbacks: tokio::sync::Mutex<ProviderBootCallbacks>,
}

impl ActiveProvider {
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

impl Drop for ActiveProvider {
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
            startup_callbacks: StartupCallbacks::default(),
        }
    }

    fn providers(&self) -> std::sync::MutexGuard<'_, Vec<Arc<ActiveProvider>>> {
        self.providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn boot_snapshot(&self) -> Vec<Arc<ActiveProvider>> {
        self.providers().clone()
    }

    pub(crate) fn take_active(&self) -> Vec<Arc<ActiveProvider>> {
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
    live: Option<LiveBoot>,
) -> BoxFuture<'a, Result<ProviderId>> {
    Box::pin(async move {
        let id = provider.id();
        if container.is_terminated() {
            return Err(Error::custom("container_closed", "容器已关闭"));
        }
        // 测试清单的移除约束覆盖同一次启动中的嵌套 force；运行期显式注册沿原 App API。
        if live.is_none() && registry.removed.contains(&id) {
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
                live_boot: live.clone(),
                provider_callbacks: &mut callbacks,
            })
            .await?;
        registry.verify_nested(id, &nested, live.is_none())?;
        let entry = Arc::new(ActiveProvider {
            id,
            provider: Some(provider),
            callbacks: tokio::sync::Mutex::new(callbacks),
        });
        let old = {
            let mut providers = registry.providers();
            if let Some(index) = providers.iter().position(|active| active.id == id) {
                Some(std::mem::replace(&mut providers[index], Arc::clone(&entry)))
            } else {
                providers.push(Arc::clone(&entry));
                None
            }
        };
        drop(guard);
        if old.is_some_and(ActiveProvider::dispose) {
            return Err(Error::custom(
                "resource_task_interrupted",
                "Provider 已替换，但原实例或启动回调析构异常",
            ));
        }
        if let Some(live) = live {
            let mut finalizers: Vec<Finalizer> = Vec::new();
            entry
                .boot(&mut BootContext {
                    console_command: None,
                    container,
                    config,
                    resources: live.resources,
                    required: &mut required,
                    extensions: &live.extensions,
                    commands: &live.commands,
                    finalizers: &mut finalizers,
                    startup_callbacks: &registry.startup_callbacks,
                    provider_callbacks: None,
                })
                .await?;
            finish_boot(container, finalizers, required).await?;
        } else {
            // 嵌套 required 在父回调原调用位置合并，最终仍在统一启动校验中解析。
            deferred_required.extend(required);
        }
        Ok(id)
    })
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;
