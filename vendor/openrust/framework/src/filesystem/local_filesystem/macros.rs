use super::{FileError, FileResult, Filesystem};
use std::{
    any::{Any, type_name},
    sync::Arc,
};
type Arguments = Box<dyn Any + Send + Sync>;
type Callback = dyn Fn(&Filesystem, Arguments) -> FileResult<Arguments> + Send + Sync;
/// 带类型校验的文件宏，可返回任意拥有所有权的 Rust 值。
#[derive(Clone)]
pub struct FileMacro {
    callback: Arc<Callback>,
    output: &'static str,
}
impl FileMacro {
    pub fn new<A: Any + Send + Sync, T: Any + Send + Sync>(
        callback: impl Fn(&Filesystem, A) -> FileResult<T> + Send + Sync + 'static,
    ) -> Self {
        Self {
            output: type_name::<T>(),
            callback: Arc::new(move |filesystem, arguments| {
                let arguments = arguments
                    .downcast::<A>()
                    .map_err(|_| FileError::MacroType {
                        name: "参数".into(),
                        expected: type_name::<A>(),
                        actual: "调用参数类型不符",
                    })?;
                callback(filesystem, *arguments).map(|value| Box::new(value) as Arguments)
            }),
        }
    }
    pub(super) fn invoke<T: Any + Send + Sync, A: Any + Send + Sync>(
        &self,
        filesystem: &Filesystem,
        name: &str,
        arguments: A,
    ) -> FileResult<T> {
        let result = (self.callback)(filesystem, Box::new(arguments))?;
        result
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| FileError::MacroType {
                name: name.to_owned(),
                expected: type_name::<T>(),
                actual: self.output,
            })
    }
}
impl Filesystem {
    pub fn r#macro<A: Any + Send + Sync, T: Any + Send + Sync>(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&Self, A) -> FileResult<T> + Send + Sync + 'static,
    ) {
        self.mixin([(name, FileMacro::new(callback))], true)
    }
    pub fn mixin<K: Into<String>>(
        &self,
        macros: impl IntoIterator<Item = (K, FileMacro)>,
        replace: bool,
    ) {
        use std::collections::hash_map::Entry;
        for (name, callback) in macros {
            let mut registry = self
                .macros
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let retired = match registry.entry(name.into()) {
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
    pub fn macro_call<T: Any + Send + Sync, A: Any + Send + Sync>(
        &self,
        name: &str,
        arguments: A,
    ) -> FileResult<T> {
        let callback = self
            .macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
            .ok_or_else(|| FileError::UnknownMacro(name.to_owned()))?;
        callback.invoke(self, name, arguments)
    }
}
