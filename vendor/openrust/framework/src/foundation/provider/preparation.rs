//! 外部包的预装配阶段；只构造唯一 Provider 清单和容器绑定。

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::ProviderEntry;
use crate::container::{ContainerBuilder, Resolver};
use crate::core::Result;
use crate::foundation::ProviderBuilder;
use crate::foundation::ProviderId;
use crate::support::BoxFuture;
use crate::support::ServiceProvider;

/// 在任何 Provider 回调前展开外部包的装配声明。
///
/// 返回项保留声明槽位，尾接项在全部显式 Provider 后执行；不建立第二套启动引擎。
/// 实现不得在此阶段启动任务或执行 Provider 的 register/boot。
pub trait ProviderRegistration: Any + Send + Sync {
    /// 校验声明、登记初始绑定，并返回当前位置的 Provider。
    fn prepare(
        self: Box<Self>,
        context: &mut ProviderPreparation<'_>,
    ) -> Result<Vec<Box<dyn ServiceProvider>>>;
}

/// 预装配可访问的受限清单与初始绑定接口。
pub struct ProviderPreparation<'a> {
    bindings: &'a mut ContainerBuilder,
    removed: &'a HashSet<ProviderId>,
    tail: &'a mut Vec<ProviderEntry>,
    conditional: &'a mut Vec<(Box<dyn ServiceProvider>, ProviderId)>,
}

impl ProviderPreparation<'_> {
    /// 使用同一根容器登记单例；正式 register 前已可解析。
    pub fn singleton<T, F>(&mut self, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.bindings.singleton::<T, F>(factory);
        self
    }

    /// 将子 Provider 追加到全部手工清单之后；显式移除继续生效。
    pub fn append_provider(&mut self, provider: Box<dyn ServiceProvider>) -> &mut Self {
        if !self.removed.contains(&provider.id()) {
            self.tail.push(ProviderEntry {
                provider,
                force: false,
            });
        }
        self
    }

    /// 全部装配声明展开后，指定身份未被移除或登记时才前置默认项。
    ///
    /// 判断包含后续声明的顶层项与尾接项，不把可能嵌套的身份当作实际登记。
    pub fn prepend_provider_unless_present(
        &mut self,
        provider: Box<dyn ServiceProvider>,
        unless: ProviderId,
    ) -> &mut Self {
        self.conditional.push((provider, unless));
        self
    }

    /// 应用是否明确移除了给定 Provider 身份。
    pub fn is_provider_removed(&self, id: ProviderId) -> bool {
        self.removed.contains(&id)
    }
}

struct RegistrationMarker<R>(std::marker::PhantomData<fn() -> R>);
impl<R: ProviderRegistration> ServiceProvider for RegistrationMarker<R> {}

impl ProviderBuilder {
    /// 安装外部包的预装配声明；同具体类型重配将旧槽移到本次调用位置。
    pub fn registration<R: ProviderRegistration>(&mut self, registration: R) -> &mut Self {
        let id = ProviderId::of::<RegistrationMarker<R>>();
        self.retain_entries(|entry| entry.provider.id() != id);
        self.registrations.insert(id, Box::new(registration));
        self.provider(RegistrationMarker::<R>(std::marker::PhantomData))
    }

    /// 在启动前修改已安装的同类型声明；不改变其槽位。
    pub fn registration_mut<R: ProviderRegistration>(&mut self) -> Option<&mut R> {
        let registration = self
            .registrations
            .get_mut(&ProviderId::of::<RegistrationMarker<R>>())?;
        (registration.as_mut() as &mut dyn Any).downcast_mut::<R>()
    }
}

pub(super) fn expand(
    providers: Vec<ProviderEntry>,
    mut registrations: HashMap<ProviderId, Box<dyn ProviderRegistration>>,
    bindings: &mut ContainerBuilder,
    removed: &HashSet<ProviderId>,
    bootstrap: &[ProviderEntry],
) -> Result<Vec<ProviderEntry>> {
    let mut expanded = Vec::new();
    let mut tail = Vec::new();
    let mut conditional = Vec::new();
    for entry in providers {
        if let Some(registration) = registrations.remove(&entry.provider.id()) {
            let mut context = ProviderPreparation {
                bindings,
                removed,
                tail: &mut tail,
                conditional: &mut conditional,
            };
            for provider in registration.prepare(&mut context)? {
                if !removed.contains(&provider.id()) {
                    expanded.push(ProviderEntry {
                        provider,
                        force: false,
                    });
                }
            }
        } else {
            expanded.push(entry);
        }
    }
    expanded.extend(tail);
    let present: HashSet<_> = bootstrap
        .iter()
        .chain(expanded.iter())
        .map(|entry| entry.provider.id())
        .collect();
    let mut prepend = Vec::new();
    for (provider, unless) in conditional {
        if !removed.contains(&unless)
            && !removed.contains(&provider.id())
            && !present.contains(&unless)
        {
            prepend.push(ProviderEntry {
                provider,
                force: false,
            });
        }
    }
    prepend.extend(expanded);
    Ok(prepend)
}

#[cfg(test)]
#[path = "preparation_tests.rs"]
mod tests;
