use super::{CloudError, CloudManager, CloudResult};
use std::{
    any::{Any, type_name},
    sync::Arc,
};
type Arguments = Box<dyn Any>;
type Callback =
    dyn Fn(&CloudManager, &str, Arguments, &'static str) -> CloudResult<Arguments> + Send + Sync;
/// 带类型校验的托管服务宏，可返回任意拥有所有权的 Rust 值。
#[derive(Clone)]
pub struct CloudMacro {
    callback: Arc<Callback>,
    output: &'static str,
}
impl CloudMacro {
    pub fn new<A: Any, T: Any>(
        callback: impl Fn(&CloudManager, A) -> CloudResult<T> + Send + Sync + 'static,
    ) -> Self {
        Self {
            output: type_name::<T>(),
            callback: Arc::new(move |cloud, name, arguments, actual| {
                let arguments = arguments
                    .downcast::<A>()
                    .map_err(|_| CloudError::MacroType {
                        name: name.into(),
                        expected: type_name::<A>(),
                        actual,
                    })?;
                callback(cloud, *arguments).map(|value| Box::new(value) as Arguments)
            }),
        }
    }
    pub(super) fn invoke<T: Any, A: Any>(
        &self,
        cloud: &CloudManager,
        name: &str,
        arguments: A,
    ) -> CloudResult<T> {
        let result = (self.callback)(cloud, name, Box::new(arguments), type_name::<A>())?;
        result
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| CloudError::MacroType {
                name: name.to_owned(),
                expected: type_name::<T>(),
                actual: self.output,
            })
    }
}
impl CloudManager {
    pub fn r#macro<A: Any, T: Any>(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&Self, A) -> CloudResult<T> + Send + Sync + 'static,
    ) {
        self.mixin([(name, CloudMacro::new(callback))], true)
    }
    pub fn mixin<K: Into<String>>(
        &self,
        macros: impl IntoIterator<Item = (K, CloudMacro)>,
        replace: bool,
    ) {
        use std::collections::hash_map::Entry;
        for (name, callback) in macros {
            let name = name.into();
            let mut registry = self
                .macros
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let retired = match registry.entry(name) {
                Entry::Vacant(entry) => {
                    entry.insert(callback);
                    None
                }
                Entry::Occupied(mut entry) if replace => Some(entry.insert(callback)),
                Entry::Occupied(_) => Some(callback),
            };
            drop(registry);
            drop(retired);
        }
    }
    pub fn has_macro(&self, name: &str) -> bool {
        self.macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(name)
    }
    pub fn flush_macros(&self) {
        let retired = std::mem::take(
            &mut *self
                .macros
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        drop(retired);
    }
    pub fn macro_call<T: Any, A: Any>(&self, name: &str, arguments: A) -> CloudResult<T> {
        let callback = self
            .macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
            .ok_or_else(|| CloudError::UnknownMacro(name.to_owned()))?;
        callback.invoke(self, name, arguments)
    }
}

impl CloudManager {
    /// 只执行本次会安装的工厂；失败保留之前已注册的宏，与原反射mixin顺序一致。
    pub fn mixin_using<K, F>(
        &self,
        factories: impl IntoIterator<Item = (K, F)>,
        replace: bool,
    ) -> CloudResult<()>
    where
        K: Into<String>,
        F: FnOnce() -> CloudResult<CloudMacro>,
    {
        for (name, factory) in factories {
            let name = name.into();
            if replace || !self.has_macro(&name) {
                self.mixin([(name, factory()?)], true);
            }
        }
        Ok(())
    }
}
