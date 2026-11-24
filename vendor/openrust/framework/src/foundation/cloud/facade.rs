use super::{CloudMacro, CloudManager, CloudResult, Queue};
use crate::{container::ServiceKey, support::facades::Facade};
use std::{any::Any, sync::Arc};
pub struct CloudFacade;
impl Facade for CloudFacade {
    type Service = CloudManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<CloudManager>()
    }
}
impl CloudFacade {
    pub fn hosted() -> CloudResult<bool> {
        Self::get_facade_root()?.hosted()
    }
    pub fn uses_managed_queues() -> CloudResult<bool> {
        Self::get_facade_root()?.uses_managed_queues()
    }
    pub fn queue() -> CloudResult<Arc<Queue>> {
        Self::get_facade_root()?.queue()
    }
    pub fn is_managed_queue(name: &str) -> CloudResult<bool> {
        Self::get_facade_root()?.is_managed_queue(name)
    }
    pub fn r#macro<A: Any, T: Any>(
        name: impl Into<String>,
        callback: impl Fn(&CloudManager, A) -> CloudResult<T> + Send + Sync + 'static,
    ) -> CloudResult<()> {
        Self::get_facade_root()?.r#macro(name, callback);
        Ok(())
    }
    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, CloudMacro)>,
        replace: bool,
    ) -> CloudResult<()> {
        Self::get_facade_root()?.mixin(macros, replace);
        Ok(())
    }
    pub fn mixin_using<K, F>(
        factories: impl IntoIterator<Item = (K, F)>,
        replace: bool,
    ) -> CloudResult<()>
    where
        K: Into<String>,
        F: FnOnce() -> CloudResult<CloudMacro>,
    {
        Self::get_facade_root()?.mixin_using(factories, replace)
    }
    pub fn has_macro(name: &str) -> CloudResult<bool> {
        Ok(Self::get_facade_root()?.has_macro(name))
    }
    pub fn flush_macros() -> CloudResult<()> {
        Self::get_facade_root()?.flush_macros();
        Ok(())
    }
    pub fn macro_call<T: Any, A: Any>(name: &str, arguments: A) -> CloudResult<T> {
        Self::get_facade_root()?.macro_call(name, arguments)
    }
}
