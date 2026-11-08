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
    providers: Vec<ActiveProvider>,
    registering: Arc<Mutex<HashSet<TypeId>>>,
    removed: HashSet<ProviderId>,
    pub(crate) startup_callbacks: StartupCallbacks,
}

pub(crate) struct ActiveProvider {
    provider: Box<dyn ServiceProvider>,
    callbacks: ProviderBootCallbacks,
}

impl ActiveProvider {
    pub(super) async fn boot(&mut self, context: &mut BootContext<'_>) -> Result<()> {
        context
            .boot_provider(self.provider.as_ref(), &mut self.callbacks)
            .await
    }

    pub(crate) fn dispose(mut self) -> bool {
        let callbacks_failed = self.callbacks.dispose();
        let provider_failed =
            crate::foundation::lifecycle::disposal::dispose(self.provider, None).seen;
        callbacks_failed || provider_failed
    }
}

impl ProviderRegistry {
    pub(crate) fn new(removed: HashSet<ProviderId>) -> Self {
        Self {
            providers: Vec::new(),
            registering: Arc::default(),
            removed,
            startup_callbacks: StartupCallbacks::default(),
        }
    }

    pub(super) fn boot_parts(&mut self) -> (&mut [ActiveProvider], &mut StartupCallbacks) {
        (&mut self.providers, &mut self.startup_callbacks)
    }

    pub(crate) fn take_active(&mut self) -> Vec<ActiveProvider> {
        std::mem::take(&mut self.providers)
    }

    fn verify_nested(
        &self,
        parent: ProviderId,
        children: &[ProviderId],
        startup: bool,
    ) -> Result<()> {
        for child in children {
            if startup && self.removed.contains(child) {
                continue;
            }
            if !self
                .providers
                .iter()
                .any(|active| active.provider.id() == *child)
            {
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
pub(crate) struct LiveBoot<'a> {
    pub(crate) resources: ResourceRegistrar,
    pub(crate) extensions: &'a mut Extensions,
    pub(crate) commands: &'a CommandRegistry,
}

impl LiveBoot<'_> {
    pub(crate) fn reborrow(&mut self) -> LiveBoot<'_> {
        LiveBoot {
            resources: self.resources.clone(),
            extensions: &mut *self.extensions,
            commands: self.commands,
        }
    }

    pub(crate) async fn booted(
        &mut self,
        container: &Container,
        config: &mut ConfigRepository,
        startup_callbacks: &mut StartupCallbacks,
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
            extensions: self.extensions,
            commands: self.commands,
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
    registry: &'a mut ProviderRegistry,
    provider: Box<dyn ServiceProvider>,
    force: bool,
    container: &'a Container,
    config: &'a mut ConfigRepository,
    deferred_required: &'a mut Vec<Required>,
    mut live: Option<LiveBoot<'a>>,
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
        let previous = registry
            .providers
            .iter()
            .position(|active| active.provider.id() == id);
        if previous.is_some() && !force {
            return Ok(id);
        }
        let ready = registry
            .providers
            .iter()
            .map(|active| active.provider.id().type_id)
            .collect();
        check_dependencies(provider.as_ref(), &ready)?;
        let nested = provider.nested_providers();
        let guard = RegistrationGuard::enter(registry, id)?;
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
                live_boot: live.as_mut().map(LiveBoot::reborrow),
                provider_callbacks: &mut callbacks,
            })
            .await?;
        registry.verify_nested(id, &nested, live.is_none())?;
        drop(guard);
        let entry = ActiveProvider {
            provider,
            callbacks,
        };
        let index = if let Some(index) = previous {
            let old = std::mem::replace(&mut registry.providers[index], entry);
            if old.dispose() {
                return Err(Error::custom(
                    "resource_task_interrupted",
                    "Provider 已替换，但原实例或启动回调析构异常",
                ));
            }
            index
        } else {
            let index = registry.providers.len();
            registry.providers.push(entry);
            index
        };
        if let Some(live) = live {
            let mut finalizers: Vec<Finalizer> = Vec::new();
            registry.providers[index]
                .boot(&mut BootContext {
                    console_command: None,
                    container,
                    config,
                    resources: live.resources,
                    required: &mut required,
                    extensions: live.extensions,
                    commands: live.commands,
                    finalizers: &mut finalizers,
                    startup_callbacks: &mut registry.startup_callbacks,
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
