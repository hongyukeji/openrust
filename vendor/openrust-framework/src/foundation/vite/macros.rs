use super::{Vite, ViteError, ViteResult};
use std::{
    any::{Any, type_name},
    sync::Arc,
};
type Arguments = Box<dyn Any>;
type Callback = dyn Fn(&Vite, &str, Arguments, &'static str) -> ViteResult<Arguments> + Send + Sync;
/// 带类型校验的资源宏，可返回任意拥有所有权的 Rust 值。
#[derive(Clone)]
pub struct ViteMacro {
    callback: Arc<Callback>,
    output: &'static str,
}
impl ViteMacro {
    pub fn new<A: Any, T: Any>(
        callback: impl Fn(&Vite, A) -> ViteResult<T> + Send + Sync + 'static,
    ) -> Self {
        Self {
            output: type_name::<T>(),
            callback: Arc::new(move |vite, name, arguments, actual| {
                let arguments = arguments
                    .downcast::<A>()
                    .map_err(|_| ViteError::MacroType {
                        name: name.into(),
                        expected: type_name::<A>(),
                        actual,
                    })?;
                callback(vite, *arguments).map(|value| Box::new(value) as Arguments)
            }),
        }
    }
    pub(super) fn invoke<T: Any, A: Any>(
        &self,
        vite: &Vite,
        name: &str,
        arguments: A,
    ) -> ViteResult<T> {
        let result = (self.callback)(vite, name, Box::new(arguments), type_name::<A>())?;
        result
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| ViteError::MacroType {
                name: name.to_owned(),
                expected: type_name::<T>(),
                actual: self.output,
            })
    }
}
impl Vite {
    pub fn r#macro<A: Any, T: Any>(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&Self, A) -> ViteResult<T> + Send + Sync + 'static,
    ) {
        self.mixin([(name, ViteMacro::new(callback))], true)
    }
    pub fn mixin<K: Into<String>>(
        &self,
        macros: impl IntoIterator<Item = (K, ViteMacro)>,
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
    pub fn macro_call<T: Any, A: Any>(&self, name: &str, arguments: A) -> ViteResult<T> {
        let callback = self
            .macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
            .ok_or_else(|| ViteError::UnknownMacro(name.to_owned()))?;
        callback.invoke(self, name, arguments)
    }
}

impl Vite {
    /// 只执行本次会安装的工厂；失败保留之前已注册的宏，与原反射mixin顺序一致。
    pub fn mixin_using<K, F>(
        &self,
        factories: impl IntoIterator<Item = (K, F)>,
        replace: bool,
    ) -> ViteResult<()>
    where
        K: Into<String>,
        F: FnOnce() -> ViteResult<ViteMacro>,
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
