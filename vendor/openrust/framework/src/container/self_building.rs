//! 编译类的自构造方法与方法绑定。

use super::attributes::retire_registration;
use super::{
    Arc, BoxFuture, Container, ContainerBuilder, Factory, HashMap, ResolvedArgs, Resolver, Result,
    ServiceKey, container_error, erase, lock,
};
use crate::foundation::lifecycle::{CallbackDisposal, callback_future};

/// 通过独立的 `new_instance` 方法构造类。方法实参由编译描述符声明。
pub trait SelfBuilding: Sized + Send + Sync + 'static {
    /// 使用当前容器与已注入的实参创建对象。外层构造覆盖不作为本方法实参。
    fn new_instance(
        args: ResolvedArgs<'_>,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<Arc<Self>>>;
}

pub(super) type SelfBuildingBindings = HashMap<ServiceKey, Factory>;

fn method<T: SelfBuilding, F>(binding: F) -> Factory
where
    F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
{
    let binding = Arc::new(binding);
    Arc::new(move |resolver| {
        let guarded = callback_future(
            resolver.callback_disposal(),
            Arc::clone(&binding),
            resolver,
            Ok(()),
            None,
            |binding, resolver, ()| binding(resolver),
        );
        Box::pin(async move { guarded.await.map(erase) })
    })
}

impl ContainerBuilder {
    /// 绑定类的 `new_instance` 方法，替换该方法及它的参数注入。
    pub fn bind_new_instance<T: SelfBuilding>(
        &mut self,
        binding: impl Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    ) -> Result<()> {
        let previous = self
            .self_building
            .insert(ServiceKey::typed::<T>(), method(binding));
        retire_registration(&CallbackDisposal::new(None, None), previous, Ok(()))
    }
}

impl Container {
    /// 绑定当前容器中的 `new_instance` 方法。后登记覆盖同类的既有方法绑定。
    pub fn bind_new_instance<T: SelfBuilding>(
        &self,
        binding: impl Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    ) -> Result<()> {
        let binding = method(binding);
        let mut state = lock(&self.inner.state);
        let disposal = CallbackDisposal::new(None, Some(state.root_resources.registrar()));
        if state.terminated {
            drop(state);
            return retire_registration(
                &disposal,
                binding,
                Err(container_error("container_closed", "容器已关闭")),
            );
        }
        let previous = state
            .self_building
            .insert(ServiceKey::typed::<T>(), binding);
        drop(state);
        retire_registration(&disposal, previous, Ok(()))
    }
}
