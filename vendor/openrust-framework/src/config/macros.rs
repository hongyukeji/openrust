use super::{ConfigError, Repository};
use crate::contracts::{config::Repository as Contract, container::Container as _};
use std::{
    any::{Any, type_name},
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub type ConfigMacroArguments = Box<dyn Any>;
type Callback = dyn Fn(
        &dyn Contract,
        &str,
        ConfigMacroArguments,
        &'static str,
    ) -> Result<ConfigMacroOutput, ConfigError>
    + Send
    + Sync;
/// 回调长期保存在线程安全注册表，单次调用参数和结果不要求 Send 或 Clone。
#[derive(Clone)]
pub struct ConfigMacro(Arc<Callback>);
pub struct ConfigMacroOutput {
    value: Box<dyn Any>,
    actual: &'static str,
}
impl ConfigMacroOutput {
    pub fn into_value<T: Any>(self, name: &str) -> Result<T, ConfigError> {
        self.value
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| ConfigError::MacroType {
                name: name.into(),
                expected: type_name::<T>(),
                actual: self.actual,
            })
    }
}
impl ConfigMacro {
    pub fn new<A: Any, T: Any>(
        callback: impl Fn(&dyn Contract, A) -> Result<T, ConfigError> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(move |repository, name, arguments, actual| {
            let arguments = arguments
                .downcast::<A>()
                .map_err(|_| ConfigError::MacroType {
                    name: name.into(),
                    expected: type_name::<A>(),
                    actual,
                })?;
            callback(repository, *arguments).map(|value| ConfigMacroOutput {
                value: Box::new(value),
                actual: type_name::<T>(),
            })
        }))
    }
}
/// 每个应用拥有独立注册表；独立仓储在应用上下文外使用自己的注册表。
#[derive(Default)]
pub struct ConfigMacroRegistry(RwLock<HashMap<String, ConfigMacro>>);
impl ConfigMacroRegistry {
    pub fn register(&self, name: String, callback: ConfigMacro) {
        let retired = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name, callback);
        drop(retired);
    }
    pub fn has(&self, name: &str) -> bool {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(name)
    }
    pub fn clear(&self) {
        let retired = std::mem::take(
            &mut *self
                .0
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        drop(retired);
    }
    pub fn invoke(
        &self,
        context: &dyn Contract,
        name: &str,
        arguments: ConfigMacroArguments,
        actual: &'static str,
    ) -> Result<ConfigMacroOutput, ConfigError> {
        let callback = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
            .ok_or_else(|| ConfigError::UnknownMacro(name.into()))?;
        (callback.0)(context, name, arguments, actual)
    }
}
pub(crate) fn current_registry() -> Result<Arc<ConfigMacroRegistry>, ConfigError> {
    crate::support::facades::current_container()
        .ok_or(ConfigError::MacroContextUnavailable)?
        .make()
        .map_err(Into::into)
}
impl Repository {
    pub fn r#macro<A: Any, T: Any>(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&dyn Contract, A) -> Result<T, ConfigError> + Send + Sync + 'static,
    ) -> Result<(), ConfigError> {
        self.register_macro(&name.into(), ConfigMacro::new(callback))
    }
    pub fn mixin<K: Into<String>>(
        &self,
        macros: impl IntoIterator<Item = (K, ConfigMacro)>,
        replace: bool,
    ) -> Result<(), ConfigError> {
        for (name, callback) in macros {
            let name = name.into();
            if replace || !self.has_macro(&name)? {
                self.register_macro(&name, callback)?;
            }
        }
        Ok(())
    }
    pub fn mixin_using<K: Into<String>, F: FnOnce() -> Result<ConfigMacro, ConfigError>>(
        &self,
        factories: impl IntoIterator<Item = (K, F)>,
        replace: bool,
    ) -> Result<(), ConfigError> {
        for (name, factory) in factories {
            let name = name.into();
            if replace || !self.has_macro(&name)? {
                self.register_macro(&name, factory()?)?;
            }
        }
        Ok(())
    }
    pub fn has_macro(&self, name: &str) -> Result<bool, ConfigError> {
        Contract::has_macro(self, name)
    }
    pub fn flush_macros(&self) -> Result<(), ConfigError> {
        Contract::flush_macros(self)
    }
    pub fn macro_call<T: Any, A: Any>(&self, name: &str, arguments: A) -> Result<T, ConfigError> {
        self.invoke_macro(self, name, Box::new(arguments), type_name::<A>())?
            .into_value(name)
    }
}
